import { Logger } from '@lib/logging';
import { drainAllBufferedSinks } from '@lib/logging/primitives';

type GlobalHandler = (error: unknown, isFatal?: boolean) => void;

type CrashTarget = {
  ErrorUtils?: { getGlobalHandler(): GlobalHandler; setGlobalHandler(handler: GlobalHandler): void };
  HermesInternal?: {
    enablePromiseRejectionTracker?: (options: {
      allRejections: boolean;
      onUnhandled: (id: number, reason: unknown) => void;
      onHandled: () => void;
    }) => void;
  };
  addEventListener?: (type: string, listener: (event: { error?: unknown; message?: string; reason?: unknown }) => void) => void;
};

// React Native's fatal handler ends the process before a queued POST leaves, so a fatal error waits for the crash
// drain's sends this long at most; a second fatal error in that window is logged and not passed on
const FATAL_DRAIN_MS = 2000;

/**
 * Every uncaught error and unhandled rejection is logged as `app.crash` before the earlier handler (React Native's red box,
 * the browser's console) sees it: React Native's global handler and Hermes's rejection tracker on a device, the window's
 * `error` and `unhandledrejection` events on the web.
 */
export function installCrashHandler(target: CrashTarget = globalThis as CrashTarget): void {
  // the buffered sinks flush once a second; a crash may be the app's last chance to write
  const crash = (kind: 'error' | 'rejection', error: unknown, fatal = false, drainMs?: number): Promise<void> => {
    // the message as a plain field: a console reads a nested error as "Object"
    Logger.error('app.crash', { kind, fatal, message: messageOf(error), error });
    return drainAllBufferedSinks(drainMs);
  };
  const utils = target.ErrorUtils;
  if (utils) {
    const earlier = utils.getGlobalHandler();
    let fatalPending = false;
    utils.setGlobalHandler((error, isFatal) => {
      if (isFatal !== true) {
        try {
          void crash('error', error);
        } finally {
          earlier(error, isFatal);
        }
        return;
      }
      if (fatalPending) {
        void crash('error', error, true);
        return;
      }
      fatalPending = true;
      let drained: Promise<void> = Promise.resolve();
      try {
        drained = crash('error', error, true, FATAL_DRAIN_MS);
      } finally {
        void drained.then(() => {
          fatalPending = false;
          earlier(error, isFatal);
        });
      }
    });
  }
  target.HermesInternal?.enablePromiseRejectionTracker?.({
    allRejections: true,
    onUnhandled: (_id, reason) => void crash('rejection', reason),
    onHandled: () => undefined,
  });
  target.addEventListener?.('error', (event) => void crash('error', event.error ?? event.message));
  target.addEventListener?.('unhandledrejection', (event) => void crash('rejection', event.reason));
}

// an object's own toString may be missing or throw (`throw Object.create(null)`); the crash handler must not throw itself
function messageOf(error: unknown): string {
  if (error instanceof Error) return error.message;
  if ((typeof error === 'object' && error !== null) || typeof error === 'function') return Object.prototype.toString.call(error);
  return String(error);
}
