import type { RpcHandlers, Timers } from '@lib/webview-rpc';

import type { JudgeParams } from '../runner/judgeProtocol';
import type { SpawnKind } from './browserWorker';
import { locationFromStack } from './harness/errorLocation';
import { runJudge, type LineSupport } from './judgeLoop';
import { createWorkerEngine, type WorkerHandle, type WorkerSpawn } from './workerEngine';
import type { WorkerError } from './workerProtocol';

/** What the page's handlers need from the world around them, so the same handlers run in a WebView, in the browser harness and in Node tests. */
export type JsPageDeps = {
  /** the harness, as the one line that goes in front of the learner's code */
  prelude: string;
  /** the two ways the host can start a worker from a script, in the order they are tried */
  spawns: Record<SpawnKind, WorkerSpawn>;
  emit(topic: string, data?: unknown): void;
  timers: Timers;
};

/** What `boot` answers: the way workers are started here, and whether this engine reports a line the two ways the harness reads one. */
export type JsBoot = { spawn: SpawnKind; eventLines: boolean; stackLines: boolean };

const PROBE_MS = 10_000;
const KINDS: SpawnKind[] = ['blob', 'data'];

type Heard = { heard: 'message'; data: unknown; scriptName: string } | { heard: 'error'; error: WorkerError } | { heard: 'none'; why: string };

/** Starts one worker on `script`, waits for its first message or error, and ends it. */
function listen(spawn: WorkerSpawn, script: string, timers: Timers): Promise<Heard> {
  return new Promise((resolve) => {
    let worker: WorkerHandle | undefined;
    let timer: unknown;
    let settled = false;
    const settle = (heard: Heard) => {
      if (settled) return;
      settled = true;
      timers.clear(timer);
      worker?.terminate();
      resolve(heard);
    };
    try {
      worker = spawn(script, {
        message: (data) => settle({ heard: 'message', data, scriptName: worker?.scriptName ?? '' }),
        error: (error) => settle({ heard: 'error', error }),
      });
    } catch (error) {
      return settle({ heard: 'none', why: error instanceof Error ? error.message : String(error) });
    }
    if (settled) worker.terminate();
    else timer = timers.set(() => settle({ heard: 'none', why: 'it did not answer' }), PROBE_MS);
  });
}

/** An error raised on line 3 of a script: the engine reports the position of an uncaught error if it names line 3. */
async function eventsCarryLines(spawn: WorkerSpawn, timers: Timers): Promise<boolean> {
  const heard = await listen(spawn, "\n\nthrow new Error('probe');", timers);
  return heard.heard === 'error' && heard.error.line === 3;
}

/** The stack of an error made on line 2 of a script: the engine writes frames that name the script's own address and give that line. */
async function stacksCarryLines(spawn: WorkerSpawn, timers: Timers): Promise<boolean> {
  const heard = await listen(spawn, "\npostMessage((function probe() { return new Error('probe').stack; })());", timers);
  return heard.heard === 'message' && locationFromStack(heard.data, heard.scriptName, 0)?.line === 2;
}

export function createJsPageHandlers(deps: JsPageDeps): RpcHandlers {
  const { timers } = deps;
  let booted: { spawn: WorkerSpawn; lines: LineSupport } | undefined;

  return {
    ping: () => Date.now(),

    async boot() {
      const failures: string[] = [];
      for (const kind of KINDS) {
        const spawn = deps.spawns[kind];
        const heard = await listen(spawn, "postMessage('ready');", timers);
        if (heard.heard !== 'message') {
          failures.push(`${kind}: ${heard.heard === 'none' ? heard.why : heard.error.message}`);
          continue;
        }
        const lines = { event: await eventsCarryLines(spawn, timers), stack: await stacksCarryLines(spawn, timers) };
        booted = { spawn, lines };
        return { spawn: kind, eventLines: lines.event, stackLines: lines.stack } satisfies JsBoot;
      }
      throw new Error(`cannot start a worker (${failures.join('; ')})`);
    },

    judge(params) {
      if (!booted) throw new Error('the page has not booted');
      const { runId, payload } = params as JudgeParams;
      const engine = createWorkerEngine({ spawn: booted.spawn, timers });
      return runJudge({ engine, prelude: deps.prelude, lines: booted.lines, timers }, payload, (row) => deps.emit('case', { runId, row }));
    },
  };
}
