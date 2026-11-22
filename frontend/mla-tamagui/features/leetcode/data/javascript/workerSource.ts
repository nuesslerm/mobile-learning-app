import { CONFIG_KEY, LOADED_KEY, type HarnessConfig } from './workerProtocol';

/**
 * The worker's script: the harness prelude and the run's configuration on line 1, then the learner's code from the same line, so
 * line N of the learner's code is line N of the script. `firstLineOffset` is how many characters of line 1 come before the
 * learner's first one. The closing call sits on a line of its own, so a trailing comment in the learner's code cannot swallow it.
 */
export function composeWorkerScript(input: { prelude: string; config: HarnessConfig; code: string }): { script: string; firstLineOffset: number } {
  const head = `globalThis[Symbol.for(${JSON.stringify(CONFIG_KEY)})]=${JSON.stringify(input.config)};${input.prelude};`;
  const tail = `\n;globalThis[Symbol.for(${JSON.stringify(LOADED_KEY)})]()`;
  return { script: head + input.code + tail, firstLineOffset: head.length };
}
