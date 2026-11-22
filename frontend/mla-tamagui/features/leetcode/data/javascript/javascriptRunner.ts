import type { Timers } from '@lib/webview-rpc';

import { isJavaScriptLanguage } from '../../domain/run/runLanguage';
import type { PageHost } from '../runner/pageHost';
import { createPageRunner, type PageRunner, type RunnerLimits } from '../runner/pageRunner';

export type JavaScriptRunner = PageRunner;

/** A page with no runtime to download boots in a blink: it is given far less than the interpreter's two minutes. */
const BOOT_MS = 15_000;

/** Runs JavaScript on the device: the shared page runner with JavaScript's words for a page that stopped or crashed, and no files to pull. */
export function createJavaScriptRunner(config: { host: PageHost; limits?: Partial<RunnerLimits>; timers?: Timers }): JavaScriptRunner {
  return createPageRunner({
    ...config,
    engine: 'javascript',
    handles: isJavaScriptLanguage,
    messages: {
      stopped: 'The JavaScript runtime stopped while running your code (it may have run out of memory).',
      crashed: 'The JavaScript runtime crashed.',
    },
    fatal: /out of memory|maximum call stack/i,
    limits: { bootMs: BOOT_MS, ...config.limits },
  });
}
