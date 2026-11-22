import { resolve } from 'node:path';
import { Worker } from 'node:worker_threads';
import { fileURLToPath } from 'node:url';

import type { TestCase } from '../../../../domain/run/execution';
import { locationFromEvent, locationFromStack } from '../../harness/errorLocation';
import type { CaseRow, FromWorker, HarnessConfig, ToWorker, WorkerError } from '../../workerProtocol';
import { composeWorkerScript } from '../../workerSource';
import { shippedPrelude } from './buildPrelude';

const ENTRY = resolve(fileURLToPath(new URL('.', import.meta.url)), 'nodeWorkerEntry.cjs');
let spawned = 0;

export type TestWorker = {
  readonly scriptName: string;
  post(message: ToWorker): void;
  terminate(): Promise<number>;
};

export type WorkerSink = { message(message: FromWorker): void; error(error: WorkerError): void };

/** A worker thread that runs `script` as a classic script under a blob-like name, speaking the page's worker protocol. */
export function spawnNodeWorker(script: string, sink: WorkerSink): TestWorker {
  spawned += 1;
  const scriptName = `blob:test/${spawned}`;
  const worker = new Worker(ENTRY, { workerData: { script, name: scriptName } });
  worker.on('message', (data: unknown) => {
    if (data !== null && typeof data === 'object' && '__workerError' in data) sink.error((data as { __workerError: WorkerError }).__workerError);
    else sink.message(data as FromWorker);
  });
  worker.on('error', (error) => sink.error({ message: `${error.name}: ${error.message}` }));
  let ended = false;
  worker.on('exit', (code) => {
    if (!ended) sink.error({ message: `the worker exited with code ${code}` });
  });
  return {
    scriptName,
    post: (message) => worker.postMessage(message),
    terminate: () => {
      ended = true;
      return worker.terminate();
    },
  };
}

export type CasesRun = {
  /** the worker's error event, when the script did not load */
  error: (WorkerError & { at: ReturnType<typeof locationFromEvent> }) | null;
  /** the `loaded` message: the module-level output, and why no case can run */
  loaded: Extract<FromWorker, { type: 'loaded' }> | null;
  rows: (CaseRow & { located?: ReturnType<typeof locationFromStack> })[];
  begun: number[];
  firstLineOffset: number;
};

/** Loads `code` in a real worker with the shipped prelude and runs the cases one by one, waiting on the worker's own messages. */
export async function runCasesInWorker(input: { code: string; config: HarnessConfig; cases: TestCase[] }): Promise<CasesRun> {
  const { script, firstLineOffset } = composeWorkerScript({ prelude: await shippedPrelude(), config: input.config, code: input.code });
  const inbox: FromWorker[] = [];
  const waiters: (() => void)[] = [];
  const state: { failure: WorkerError | null } = { failure: null };
  const wake = () => waiters.splice(0).forEach((resolveWaiter) => resolveWaiter());
  const worker = spawnNodeWorker(script, {
    message(message) {
      inbox.push(message);
      wake();
    },
    error(error) {
      state.failure = error;
      wake();
    },
  });
  const next = async (accepts: (message: FromWorker) => boolean): Promise<FromWorker | null> => {
    for (;;) {
      const at = inbox.findIndex(accepts);
      if (at >= 0) return inbox.splice(at, 1)[0] ?? null;
      if (state.failure) return null;
      await new Promise<void>((resolveWaiter) => waiters.push(resolveWaiter));
    }
  };
  const result: CasesRun = { error: null, loaded: null, rows: [], begun: [], firstLineOffset };
  try {
    const loaded = await next((message) => message.type === 'loaded');
    if (state.failure) {
      const error: WorkerError = state.failure;
      result.error = { ...error, at: locationFromEvent({ lineno: error.line, colno: error.column }, firstLineOffset) };
      return result;
    }
    result.loaded = loaded as Extract<FromWorker, { type: 'loaded' }>;
    if (result.loaded.error) return result;
    for (const [index, testCase] of input.cases.entries()) {
      worker.post({ type: 'case', index, case: testCase });
      const begin = await next((message) => message.type === 'begin' && message.index === index);
      if (begin) result.begun.push(index);
      const row = await next((message) => message.type === 'row' && message.index === index);
      if (!row || row.type !== 'row') break;
      const located = row.row.stack === undefined ? undefined : locationFromStack(row.row.stack, worker.scriptName, firstLineOffset);
      result.rows.push({ ...row.row, ...(located ? { located } : {}) });
    }
    return result;
  } finally {
    await worker.terminate();
  }
}
