import { createRpcClient, RpcError, type RpcClient, type Timers } from '@lib/webview-rpc';
import { Logger } from '@lib/logging';

import type { RunResponse, TestResult } from '../../domain/execution';
import { CodeRunnerError, type CodeRunnerPort, type CodeRunnerStage, type RunCodeParams } from '../../domain/ports';
import { rawRunSchema, runResponseOf, type RawRun } from '../../domain/verdict';
import { JUDGE_DEFAULTS, type JudgeParams } from './judgeProtocol';
import type { PageHost, PageSession } from './pageHost';

export type RunnerLimits = {
  /** the page announces itself */
  readyMs: number;
  /** the runtime files reach the page */
  pullMs: number;
  /** the interpreter boots */
  bootMs: number;
  /** a run waits for a load that is under way before it gives up on it (the load carries on) */
  waitMs: number;
  /** the whole run, the backend's process limit: the harness stops there, and a page that has not answered `rpcSlackMs` later is thrown away */
  runMs: number;
  rpcSlackMs: number;
  /** one case, the backend's per-case limit */
  caseMs: number;
  pingMs: number;
  /** a page idle this long is pinged before it is trusted with a run */
  idleCheckMs: number;
  /** consecutive failed loads after which a discarded page is no longer reloaded on its own */
  maxLoadFailures: number;
};
export const DEFAULT_LIMITS: RunnerLimits = { readyMs: 60_000, pullMs: 60_000, bootMs: 120_000, waitMs: 45_000, runMs: JUDGE_DEFAULTS.processLimitMs, rpcSlackMs: JUDGE_DEFAULTS.rpcSlackMs, caseMs: JUDGE_DEFAULTS.timeoutMs, pingMs: 1_500, idleCheckMs: 15_000, maxLoadFailures: 3 };

export type RunnerState = 'idle' | 'loading' | 'ready' | 'running' | 'resetting' | 'failed';

export type PyodideRunner = CodeRunnerPort & {
  warmUp(): void;
  readonly state: RunnerState;
  subscribe(listener: (state: RunnerState) => void): () => void;
  /** Fails the queued runs as closed and discards the page; the runner stays usable. */
  dispose(): void;
};

type Page = { session: PageSession; client: RpcClient; stopLost: () => void };
type Job = { params: RunCodeParams; resolve: (response: RunResponse) => void; reject: (error: unknown) => void };

const isPython = (lang: string) => lang === 'python3' || lang === 'python';
const messageOf = (error: unknown) => (error instanceof Error ? error.message : String(error));
const FATAL = /fatal error|maximum call stack/i;
const realTimers: Timers = { set: (fn, ms) => setTimeout(fn, ms), clear: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>) };

/** Rejects when `ms` pass; `settle` stops the timer. */
function deadline(timers: Timers, ms: number, make: () => Error): { promise: Promise<never>; settle: () => void } {
  let handle: unknown;
  const promise = new Promise<never>((_, reject) => {
    handle = timers.set(() => reject(make()), ms);
  });
  return { promise, settle: () => timers.clear(handle) };
}

function once(client: RpcClient, topic: string, timers: Timers, ms: number, make: () => Error): Promise<void> {
  return new Promise((resolve, reject) => {
    const timer = timers.set(() => {
      stop();
      reject(make());
    }, ms);
    const stop = client.subscribe(topic, () => {
      timers.clear(timer);
      stop();
      resolve();
    });
  });
}

/**
 * Runs Python on the device behind CodeRunnerPort: one hidden page loaded once and kept warm, runs queued one at a time, each run's
 * limit enforced by the harness (per case) and here (the whole run: a page that cannot answer is thrown away and loaded again).
 */
export function createPyodideRunner(config: { host: PageHost; limits?: Partial<RunnerLimits>; timers?: Timers }): PyodideRunner {
  const { host } = config;
  const limits = { ...DEFAULT_LIMITS, ...config.limits };
  const timers = config.timers ?? realTimers;
  const queue: Job[] = [];
  const subscribers = new Set<(state: RunnerState) => void>();
  let state: RunnerState = 'idle';
  let page: Page | null = null;
  let loading: Promise<Page> | null = null;
  let failures = 0;
  let pumping = false;
  let lastUsed = Date.now();
  let nextRunId = 1;
  let generation = 0;

  const setState = (to: RunnerState, reason: string) => {
    if (to === state) return;
    Logger.info('pyodide_runner.state', { from: state, to, reason });
    state = to;
    for (const listener of [...subscribers]) listener(to);
  };

  async function load(): Promise<Page> {
    const mine = (generation += 1);
    setState('loading', 'load');
    let stage: CodeRunnerStage = 'assets';
    let client: RpcClient | undefined;
    let stopLoadLost: (() => void) | undefined;
    const started = Date.now();
    try {
      const session = await host.prepare();
      client = createRpcClient(session.transport, { timers, timeoutMs: limits.pullMs });
      const lost = new Promise<never>((_, reject) => {
        stopLoadLost = session.onLost((reason) => reject(new CodeRunnerError('unavailable', `the page was lost: ${reason}`, stage)));
      });
      // the rejection of an unused `lost` must not surface as unhandled once the load has settled
      lost.then(undefined, () => undefined);
      stage = 'page';
      const ready = once(client, 'ready', timers, limits.readyMs, () => new CodeRunnerError('unavailable', 'the page never said it was ready', 'page'));
      session.show();
      await Promise.race([ready, lost]);
      stage = 'pull';
      const pullStarted = Date.now();
      let strategy = 'pull';
      await Promise.race([
        client.call('pull', session.files, { timeoutMs: limits.pullMs }).then(undefined, async (pullError: unknown) => {
          strategy = 'chunks';
          Logger.warn('pyodide_runner.pull_failed', { message: messageOf(pullError) });
          for (const name of Object.keys(session.files) as (keyof PageSession['files'])[]) {
            for await (const piece of session.chunks(name)) await client?.call('chunk', { name, ...piece }, { timeoutMs: limits.pullMs });
          }
        }),
        lost,
      ]);
      stage = 'boot';
      const booted = await Promise.race([client.call<{ importMs: number; loadMs: number; version: string }>('boot', undefined, { timeoutMs: limits.bootMs }), lost]);
      if (mine !== generation) throw new CodeRunnerError('closed', 'the runner was reset while the page loaded');
      stopLoadLost?.();
      // a page that dies while idle is dropped, and the next run loads a fresh one
      const stopIdleLost = session.onLost((reason) => {
        if (page?.session === session && state !== 'running') discard(reason, false);
      });
      page = { session, client, stopLost: stopIdleLost };
      failures = 0;
      lastUsed = Date.now();
      Logger.info('pyodide_runner.load', { stage: 'done', ms: Date.now() - started, transferMs: Date.now() - pullStarted, strategy, importMs: booted.importMs, loadMs: booted.loadMs, version: booted.version });
      setState('ready', 'loaded');
      return page;
    } catch (error) {
      stopLoadLost?.();
      client?.close();
      // a load the runner has moved past (a reset, a dispose) must not close the page of the one that replaced it
      if (mine === generation) {
        host.close();
        failures += 1;
        setState('failed', 'load_failed');
      }
      Logger.withError(error).warn('pyodide_runner.load_failed', { stage, ms: Date.now() - started, failures });
      throw error instanceof CodeRunnerError ? error : new CodeRunnerError('unavailable', messageOf(error), stage);
    }
  }

  const ensureLoaded = (): Promise<Page> => {
    if (page) return Promise.resolve(page);
    if (!loading) {
      const attempt: Promise<Page> = load().finally(() => {
        if (loading === attempt) loading = null;
      });
      loading = attempt;
    }
    return loading;
  };

  function discard(reason: string, reload: boolean) {
    generation += 1;
    const old = page;
    page = null;
    loading = null;
    old?.stopLost();
    old?.client.close();
    host.close();
    Logger.warn('pyodide_runner.page_lost', { reason });
    setState('resetting', reason);
    if (reload && failures < limits.maxLoadFailures) {
      void ensureLoaded().then(undefined, (error: unknown) => {
        Logger.withError(error).warn('pyodide_runner.reload_failed', { failures });
      });
    } else setState('idle', 'discarded');
  }

  async function execute(params: RunCodeParams): Promise<RunResponse> {
    const wait = deadline(timers, limits.waitMs, () => new CodeRunnerError('unavailable', 'the on-device runner is still starting', 'wait'));
    let current: Page;
    try {
      current = await Promise.race([ensureLoaded(), wait.promise]);
    } finally {
      wait.settle();
    }
    if (Date.now() - lastUsed > limits.idleCheckMs) {
      const alive = await current.client.call('ping', undefined, { timeoutMs: limits.pingMs }).then(
        () => true,
        (error: unknown) => {
          Logger.withError(error).warn('pyodide_runner.ping_failed');
          return false;
        }
      );
      if (!alive) {
        discard('ping failed', false);
        current = await ensureLoaded();
      }
    }
    setState('running', 'run');
    const runId = nextRunId++;
    const started = Date.now();
    const rows = new Map<number, TestResult>();
    const stopCases = current.client.subscribe('case', (data) => {
      const event = data as { runId?: number; row?: string };
      if (event.runId !== runId || typeof event.row !== 'string') return;
      const parsed = JSON.parse(event.row) as { index: number; result: TestResult };
      rows.set(parsed.index, parsed.result);
    });
    const collected = () => [...rows.entries()].sort(([a], [b]) => a - b).map(([, row]) => row);
    const total = params.testCases.length;
    const early = (error: string, kind: 'timeout' | 'runtime'): RunResponse =>
      runResponseOf({ ok: false, error, error_kind: kind, results: collected(), total }, 'offline', { surfaceCaseError: true });
    let lostReason: string | undefined;
    let stopRunLost: (() => void) | undefined;
    const lost = new Promise<never>((_, reject) => {
      stopRunLost = current.session.onLost((reason) => {
        lostReason = reason;
        reject(new CodeRunnerError('closed', reason));
      });
    });
    lost.then(undefined, () => undefined);
    const call: JudgeParams = {
      runId,
      payload: {
        code: params.code,
        methodName: params.methodName,
        testCases: params.testCases,
        ...(params.paramTypes ? { paramTypes: params.paramTypes } : {}),
        ...(params.returnType === undefined ? {} : { returnType: params.returnType }),
        ...(params.inPlace === undefined ? {} : { inPlace: params.inPlace }),
        timeoutMs: limits.caseMs,
        processLimitMs: limits.runMs,
      },
    };
    try {
      const text = await Promise.race([current.client.call<string>('judge', call, { timeoutMs: limits.runMs + limits.rpcSlackMs }), lost]);
      const raw: RawRun = rawRunSchema.parse(JSON.parse(text));
      const answer = raw.ok ? raw : { ...raw, results: collected(), total };
      Logger.info('pyodide_runner.run', { cases: total, ms: Date.now() - started, outcome: raw.ok ? 'answered' : (raw.error_kind ?? 'error') });
      lastUsed = Date.now();
      setState('ready', 'answered');
      return runResponseOf(answer, 'offline', { surfaceCaseError: true });
    } catch (error) {
      if (lostReason !== undefined) {
        const stopped = early('The Python runtime stopped while running your code (it may have run out of memory).', 'runtime');
        discard(lostReason, true);
        return stopped;
      }
      if (error instanceof RpcError && error.code === 'timeout') {
        Logger.warn('pyodide_runner.timeout', { limitMs: limits.runMs, collected: rows.size });
        const timedOut = early('Timeout: execution exceeded process limit', 'timeout');
        discard('run timed out', true);
        return timedOut;
      }
      if (error instanceof RpcError && error.code === 'remote' && FATAL.test(error.message)) {
        const crashed = early('The Python runtime crashed, usually because of very deep recursion.', 'runtime');
        discard('interpreter crashed', true);
        return crashed;
      }
      Logger.withError(error).warn('pyodide_runner.run_failed', { cases: total });
      discard('run failed', true);
      throw error instanceof CodeRunnerError ? error : new CodeRunnerError('closed', messageOf(error));
    } finally {
      stopCases();
      stopRunLost?.();
    }
  }

  async function pump() {
    if (pumping) return;
    pumping = true;
    try {
      for (let job = queue[0]; job; job = queue[0]) {
        await execute(job.params).then(job.resolve, job.reject);
        // the queue may have been emptied and refilled meanwhile (dispose): remove this job, never a newer one
        const at = queue.indexOf(job);
        if (at >= 0) queue.splice(at, 1);
      }
    } finally {
      pumping = false;
    }
  }

  return {
    get state() {
      return state;
    },
    subscribe(listener) {
      subscribers.add(listener);
      return () => void subscribers.delete(listener);
    },
    warmUp() {
      void ensureLoaded().then(undefined, () => undefined);
    },
    run(params) {
      if (!isPython(params.lang)) {
        return Promise.resolve(runResponseOf({ ok: false, error: `Unsupported language: ${params.lang}`, error_kind: 'compile' }, 'offline'));
      }
      return new Promise<RunResponse>((resolve, reject) => {
        queue.push({ params, resolve, reject });
        void pump();
      });
    },
    dispose() {
      for (const job of queue.splice(0)) job.reject(new CodeRunnerError('closed', 'the runner was closed'));
      if (page || loading) discard('disposed', false);
      setState('idle', 'disposed');
    },
  };
}
