import type { TestResult } from '../../../../domain/run/execution';
import type { RawRun } from '../../../../domain/run/verdict';
import { createManualTimers, type ManualTimers } from '../../../../testing/manualTimers';
import type { JudgePayload } from '../../../runner/judgeProtocol';
import { runJudge, type LineSupport } from '../../judgeLoop';
import { createWorkerEngine, type WorkerSpawn } from '../../workerEngine';
import type { FromWorker } from '../../workerProtocol';
import { shippedPrelude } from './buildPrelude';
import { spawnNodeWorker } from './nodeWorkerSpawn';

export type Judged = { raw: RawRun; emitted: { index: number; result: TestResult }[] };

export type JudgeOptions = {
  spawn?: WorkerSpawn;
  timers?: ManualTimers;
  lines?: LineSupport;
};

/** Lets a test see each worker start, each message it sends as it sends it, and each worker ended: the signals a test waits on before it fires a timer. */
export function watched(
  inner: WorkerSpawn,
  watch: { spawned?(scriptName: string): void; message?(message: FromWorker): void; ended?(scriptName: string): void }
): WorkerSpawn {
  return (script, sink) => {
    const handle = inner(script, {
      message(message) {
        watch.message?.(message);
        sink.message(message);
      },
      error: sink.error,
    });
    watch.spawned?.(handle.scriptName);
    return {
      scriptName: handle.scriptName,
      post: (message) => handle.post(message),
      terminate: () => {
        handle.terminate();
        watch.ended?.(handle.scriptName);
      },
    };
  };
}

/** Runs a payload through the real judge loop and real worker threads. Timers are the test's: nothing times out unless it fires them. */
export function startJudge(payload: JudgePayload, options: JudgeOptions = {}): { done: Promise<Judged>; timers: ManualTimers } {
  const timers = options.timers ?? createManualTimers();
  const done = (async (): Promise<Judged> => {
    const emitted: Judged['emitted'] = [];
    const engine = createWorkerEngine({ spawn: options.spawn ?? spawnNodeWorker, timers });
    const text = await runJudge(
      { engine, timers, prelude: await shippedPrelude(), lines: options.lines ?? { event: true, stack: true } },
      payload,
      (row) => emitted.push(JSON.parse(row) as Judged['emitted'][number])
    );
    return { raw: JSON.parse(text) as RawRun, emitted };
  })();
  return { done, timers };
}

export const judge = (payload: JudgePayload, options?: JudgeOptions): Promise<Judged> => startJudge(payload, options).done;
