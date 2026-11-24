import type { Timers } from '@lib/webview-rpc';

import type { TestCase } from '../../domain/run/execution';
import type { CaseRow, FromWorker, ToWorker, WorkerError } from './workerProtocol';

/** One started worker. Its messages and the uncaught errors of its script go to the sink it was spawned with. */
export type WorkerHandle = {
  /** the name its stack frames carry: a blob url, or the script's name in a test */
  readonly scriptName: string;
  post(message: ToWorker): void;
  terminate(): void;
};
export type WorkerSink = { message(message: FromWorker): void; error(error: WorkerError): void };
/** Starts a worker that runs `script`; throws when the host cannot start one. */
export type WorkerSpawn = (script: string, sink: WorkerSink) => WorkerHandle;

export type LoadOutcome =
  | { kind: 'loaded'; stdout: string }
  /** the code loaded but nothing can be called: the message says what is missing */
  | { kind: 'missing'; message: string }
  /** the script failed to compile or threw at module level: the worker's own error event */
  | { kind: 'failed'; error: WorkerError }
  | { kind: 'timeout' }
  | { kind: 'died'; reason: string };

export type CaseOutcome =
  /** `scriptName` is the one the row's stack names: the worker that ran it */
  | { kind: 'row'; row: CaseRow; scriptName: string }
  | { kind: 'timeout' }
  /** the worker's error event reached the case in flight: a throw out of a timer or a callback, which the harness's own try cannot see */
  | { kind: 'raised'; error: WorkerError }
  /** no worker could be started, or the host could not talk to it */
  | { kind: 'died'; reason: string };

export type WorkerEngine = {
  /** Starts a worker on `script` and waits for its module-level code, for at most `limitMs`. */
  load(script: string, limitMs: number): Promise<LoadOutcome>;
  /** Runs one case. A case that outlasts `limitMs`, or that raises out of the harness, ends the worker: the next case starts a new one. */
  runCase(index: number, testCase: TestCase, limitMs: number): Promise<CaseOutcome>;
  /** Ends the live worker. */
  dispose(): void;
};

type Event = { type: 'message'; message: FromWorker } | { type: 'error'; error: WorkerError } | { type: 'deadline' };
type Events = ReturnType<typeof channel>;

function channel() {
  const queue: Event[] = [];
  let wake: (() => void) | null = null;
  return {
    push(event: Event) {
      queue.push(event);
      wake?.();
      wake = null;
    },
    async next(): Promise<Event> {
      for (;;) {
        const event = queue.shift();
        if (event) return event;
        await new Promise<void>((resolve) => {
          wake = resolve;
        });
      }
    },
  };
}

const reasonOf = (error: unknown) => (error instanceof Error ? error.message : String(error));

/**
 * Supervises the worker of one run. Everything a worker says, and every deadline, passes through one ordered channel per worker, so a
 * late message of a worker that was ended can never answer a case of the next one. The deadlines are the host's timers: a loop in the
 * learner's code cannot stop them, and ending the worker is the only thing that stops the loop.
 */
export function createWorkerEngine(deps: { spawn: WorkerSpawn; timers: Timers }): WorkerEngine {
  const { spawn, timers } = deps;
  let script = '';
  let live: { handle: WorkerHandle; events: Events } | null = null;

  function end() {
    live?.handle.terminate();
    live = null;
  }

  function start(): { events: Events } | { reason: string } {
    const events = channel();
    try {
      const handle = spawn(script, {
        message: (message) => events.push({ type: 'message', message }),
        error: (error) => events.push({ type: 'error', error }),
      });
      live = { handle, events };
      return { events };
    } catch (error) {
      return { reason: `cannot start a worker: ${reasonOf(error)}` };
    }
  }

  async function boot(limitMs: number): Promise<LoadOutcome> {
    const started = start();
    if ('reason' in started) return { kind: 'died', reason: started.reason };
    const { events } = started;
    const timer = timers.set(() => events.push({ type: 'deadline' }), limitMs);
    try {
      for (;;) {
        const event = await events.next();
        if (event.type === 'deadline') {
          end();
          return { kind: 'timeout' };
        }
        if (event.type === 'error') {
          end();
          return { kind: 'failed', error: event.error };
        }
        if (event.message.type !== 'loaded') continue;
        if (event.message.error !== undefined) {
          end();
          return { kind: 'missing', message: event.message.error };
        }
        return { kind: 'loaded', stdout: event.message.stdout };
      }
    } finally {
      timers.clear(timer);
    }
  }

  async function ready(limitMs: number): Promise<{ ok: true; live: NonNullable<typeof live> } | { ok: false; outcome: CaseOutcome }> {
    if (!live) {
      const again = await boot(limitMs);
      if (again.kind === 'timeout') return { ok: false, outcome: { kind: 'timeout' } };
      if (again.kind !== 'loaded') {
        const reason = again.kind === 'died' ? again.reason : again.kind === 'failed' ? again.error.message : again.message;
        return { ok: false, outcome: { kind: 'died', reason } };
      }
    }
    return live ? { ok: true, live } : { ok: false, outcome: { kind: 'died', reason: 'the worker is gone' } };
  }

  return {
    load(source, limitMs) {
      script = source;
      end();
      return boot(limitMs);
    },

    async runCase(index, testCase, limitMs) {
      const found = await ready(limitMs);
      if (!found.ok) return found.outcome;
      const { handle, events } = found.live;
      // until the worker says the case has begun, the deadline is a guard against a worker too busy to begin it
      let timer = timers.set(() => events.push({ type: 'deadline' }), limitMs);
      try {
        handle.post({ type: 'case', index, case: testCase });
        for (;;) {
          const event = await events.next();
          if (event.type === 'deadline') {
            end();
            return { kind: 'timeout' };
          }
          if (event.type === 'error') {
            end();
            return { kind: 'raised', error: event.error };
          }
          const { message } = event;
          if (message.type === 'begin' && message.index === index) {
            timers.clear(timer);
            timer = timers.set(() => events.push({ type: 'deadline' }), limitMs);
          } else if (message.type === 'row' && message.index === index) {
            return { kind: 'row', row: message.row, scriptName: handle.scriptName };
          }
        }
      } catch (error) {
        end();
        return { kind: 'died', reason: reasonOf(error) };
      } finally {
        timers.clear(timer);
      }
    },

    dispose: end,
  };
}
