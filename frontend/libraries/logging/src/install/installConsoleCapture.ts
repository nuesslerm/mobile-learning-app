// Generic console.* patcher. Hosts pass a `target` (the in-memory store or
// any sink that exposes `add(level, text, { source, context })`) and we
// route every `console.{debug,log,info,warn,error}` call into it. The
// patch is idempotent — calling `installConsoleCapture` twice is a no-op.
//
// The package deliberately does NOT capture the "original" console
// methods itself. Hosts that need the original-pre-install console (e.g.
// so a downstream transport can echo without recursing into the patched
// console) snapshot the methods BEFORE calling `installConsoleCapture`:
//
//   // host code:
//   const originalConsole = { ...console };
//   installConsoleCapture({ target: logStore, source: 'console' });
//   // pass originalConsole to ConsoleEchoTransport so it doesn't recurse

import type { AddOptions, LogLevel } from '../liveTail/index.js';

/** Anything that can receive a console capture. The `logStore` exposes
 *  exactly this shape; tests can pass a mock. */
export type ConsoleCaptureTarget = {
  add(level: LogLevel, text: string, opts?: AddOptions): unknown;
};

export type InstallConsoleCaptureOptions = {
  /** Sink that receives the captured entries. */
  target: ConsoleCaptureTarget;
  /** Authoring-module tag for entries coming from console.* (default: `'console'`). */
  source?: string;
  /** Methods to patch. Default: all five (`debug`, `log`, `info`, `warn`, `error`). */
  levels?: readonly LogLevel[];
};

const ALL_LEVELS: readonly LogLevel[] = [
  'debug',
  'log',
  'info',
  'warn',
  'error',
];

let installed = false;
let previousConsole: Record<string, (...args: unknown[]) => void> | null = null;

/** Patch the global `console` so each call routes to the target's `add()`.
 *  Idempotent — calling twice returns false on the second call without
 *  re-patching. Returns true if this was the first install. */
export function installConsoleCapture(
  opts: InstallConsoleCaptureOptions
): boolean {
  if (installed) return false;
  const source = opts.source ?? 'console';
  const levels = opts.levels ?? ALL_LEVELS;
  // Snapshot the methods we're about to replace so the host can recover
  // them after install (used by `ConsoleEchoTransport` to echo without
  // recursing).
  const c = console as unknown as Record<string, (...args: unknown[]) => void>;
  previousConsole = {};
  for (const level of levels) {
    if (typeof c[level] !== 'function') continue;
    previousConsole[level] = c[level].bind(console);
    c[level] = (...args: unknown[]) => {
      try {
        opts.target.add(level, stringifyArgs(args), { source });
      } catch {
        // logging must never throw
      }
      // Call through to the original so dev tooling (Metro, browser devtools)
      // still sees the output. Hosts that want to silence the original can
      // call `restoreConsole()` after install.
      const prev = previousConsole;
      prev?.[level]?.(...args);
    };
  }
  installed = true;
  return true;
}

/** Restore the global `console` to its pre-install state. No-op if not
 *  installed. Returns true if the restore actually happened. */
export function restoreConsole(): boolean {
  if (!installed || !previousConsole) return false;
  const c = console as unknown as Record<string, (...args: unknown[]) => void>;
  for (const [level, fn] of Object.entries(previousConsole)) {
    c[level] = fn;
  }
  installed = false;
  previousConsole = null;
  return true;
}

/** Snapshot of the console methods at install time, or `null` if not yet
 *  installed. Hosts use this to pass the pre-install console to
 *  `ConsoleEchoTransport`. */
export function getOriginalConsole(): Record<
  string,
  (...args: unknown[]) => void
> | null {
  return previousConsole;
}

function stringifyArg(arg: unknown): string {
  if (typeof arg === 'string') return arg;
  if (arg instanceof Error) return arg.stack ?? `${arg.name}: ${arg.message}`;
  try {
    return JSON.stringify(arg);
  } catch {
    return String(arg);
  }
}

function stringifyArgs(args: readonly unknown[]): string {
  return args.map(stringifyArg).join(' ');
}

/** Test-only: reset the install state. */
export function __resetConsoleCapture(): void {
  if (installed) {
    const c = console as unknown as Record<
      string,
      (...args: unknown[]) => void
    >;
    const prev = previousConsole;
    if (prev) {
      for (const [level, fn] of Object.entries(prev)) {
        c[level] = fn;
      }
    }
  }
  installed = false;
  previousConsole = null;
}
