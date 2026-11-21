import { createRpcServer, type RpcServer } from '@lib/webview-rpc';
import { createLoopbackPair } from '@lib/webview-rpc/testing';

import type { TestResult } from '../domain/run/execution';
import { CodeRunnerError } from '../domain/ports';
import type { RawRun } from '../domain/run/verdict';
import type { JudgeParams } from '../data/pyodide/judgeProtocol';
import type { PyodideFileName } from '../data/pyodide/pageHandlers';
import type { PageHost, PageSession } from '../data/pyodide/pageHost';

/** What a scripted judge returns to make the page hang: it never answers, as a runaway loop would. */
export const HANG: unique symbol = Symbol('hang');

export type FakePageScript = {
  readyAfterMs?: number;
  assetsFail?: string;
  pullFails?: boolean;
  bootFails?: string;
  bootMs?: number;
  /** answers a run; the default passes every case. `load` counts the pages loaded so far, so a script can hang only the first. */
  judge?: (call: JudgeParams, load: number) => RawRun | Promise<RawRun> | typeof HANG;
};

export type FakePageHost = PageHost & {
  readonly loads: number;
  readonly closes: number;
  readonly calls: readonly { load: number; method: string; params: unknown }[];
  /** the native view reports the page dead */
  crash(reason?: string): void;
  /** changes how the next page, or the next run, behaves */
  script(next: Partial<FakePageScript>): void;
};

const FILES: Record<PyodideFileName, string> = { asm: 'fake://asm', lock: 'fake://lock', wasm: 'fake://wasm', stdlib: 'fake://stdlib' };
const later = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

const row = (pass: boolean, call: JudgeParams, index: number, over: Partial<TestResult> = {}): TestResult => ({
  pass,
  input: call.payload.testCases[index]?.inputs ?? {},
  actual: pass ? call.payload.testCases[index]?.expected : null,
  expected: call.payload.testCases[index]?.expected,
  stdout: '',
  stderr: '',
  error: null,
  elapsed_ms: 1,
  ...over,
});

/** Pure builders for what the page answers. */
export const answers = {
  allPass: (call: JudgeParams): RawRun => ({ ok: true, results: call.payload.testCases.map((_, i) => row(true, call, i)) }),
  wrongAnswer: (call: JudgeParams, failing = 0): RawRun => ({ ok: true, results: call.payload.testCases.map((_, i) => row(i !== failing, call, i)) }),
  /** what the judge answers for a typed case: its value, with `pass` false because there was nothing to compare (expected is null) */
  custom: (call: JudgeParams, actual: unknown): RawRun => ({ ok: true, results: call.payload.testCases.map((_, i) => row(false, call, i, { actual })) }),
  caseError: (call: JudgeParams, error: string, at: { line: number; column?: number }, failing = 0): RawRun => ({
    ok: true,
    results: call.payload.testCases.map((_, i) => (i === failing ? row(false, call, i, { error, error_at: at, actual: null }) : row(true, call, i))),
  }),
  compileError: (error: string, at?: { line: number; column?: number }): RawRun => ({ ok: false, results: [], error, error_kind: 'compile', ...(at ? { error_at: at } : {}) }),
};

/** A page host whose page is a real rpc server on a loopback answering from a script: the runner speaks the real wire format to it. */
export function createFakePageHost(initial: FakePageScript = {}): FakePageHost {
  let scripted: FakePageScript = initial;
  let loads = 0;
  let closes = 0;
  const calls: { load: number; method: string; params: unknown }[] = [];
  let lostListeners = new Set<(reason: string) => void>();
  const servers: RpcServer[] = [];

  return {
    get loads() {
      return loads;
    },
    get closes() {
      return closes;
    },
    calls,
    crash: (reason = 'the page crashed') => {
      for (const listener of [...lostListeners]) listener(reason);
    },
    script: (next) => {
      scripted = { ...scripted, ...next };
    },
    async prepare(): Promise<PageSession> {
      if (scripted.assetsFail) throw new CodeRunnerError('unavailable', scripted.assetsFail, 'assets');
      loads += 1;
      const load = loads;
      const pair = createLoopbackPair();
      lostListeners = new Set();
      const record = (method: string, handler: (params: unknown) => unknown) => (params: unknown) => {
        calls.push({ load, method, params });
        return handler(params);
      };
      const server = createRpcServer(pair.page, {
        ping: record('ping', () => Date.now()),
        pull: record('pull', () => {
          if (scripted.pullFails) throw new Error('the page could not read the files');
          return { ms: 1 };
        }),
        chunk: record('chunk', () => null),
        boot: record('boot', async () => {
          if (scripted.bootMs) await later(scripted.bootMs);
          if (scripted.bootFails) throw new Error(scripted.bootFails);
          return { importMs: 1, loadMs: 1, version: 'fake' };
        }),
        memory: record('memory', () => ({ jsHeapMb: null, wasmHeapMb: null })),
        judge: record('judge', async (params) => {
          const call = params as JudgeParams;
          const answer = (scripted.judge ?? ((c: JudgeParams) => answers.allPass(c)))(call, load);
          if (answer === HANG) return new Promise<string>(() => undefined);
          const raw = await answer;
          if (raw.ok) raw.results?.forEach((result, index) => server.emit('case', { runId: call.runId, row: JSON.stringify({ index, result }) }));
          return JSON.stringify(raw);
        }),
      });
      servers.push(server);
      return {
        transport: pair.host,
        files: FILES,
        show: () => void later(scripted.readyAfterMs ?? 0).then(() => server.emit('ready')),
        async *chunks() {
          yield { index: 0, total: 2, data: '' };
          yield { index: 1, total: 2, data: '' };
        },
        onLost(listener) {
          lostListeners.add(listener);
          return () => void lostListeners.delete(listener);
        },
      };
    },
    close() {
      closes += 1;
      for (const server of servers.splice(0)) server.close();
    },
  };
}
