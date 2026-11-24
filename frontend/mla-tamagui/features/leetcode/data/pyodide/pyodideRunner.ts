import type { Timers } from '@lib/webview-rpc';

import { isPythonLanguage } from '../../domain/run/runLanguage';
import type { PageHost } from '../runner/pageHost';
import { createPageRunner, type PageRunner, type RunnerLimits } from '../runner/pageRunner';

export type { RunnerLimits, RunnerState } from '../runner/pageRunner';
export type PyodideRunner = PageRunner;

/** Runs Python on the device: the shared page runner with Python's words for a page that stopped or crashed. */
export function createPyodideRunner(config: { host: PageHost; limits?: Partial<RunnerLimits>; timers?: Timers }): PyodideRunner {
  return createPageRunner({
    ...config,
    engine: 'pyodide',
    handles: isPythonLanguage,
    messages: {
      stopped: 'The Python runtime stopped while running your code (it may have run out of memory).',
      crashed: 'The Python runtime crashed, usually because of very deep recursion.',
    },
    fatal: /fatal error|maximum call stack/i,
  });
}
