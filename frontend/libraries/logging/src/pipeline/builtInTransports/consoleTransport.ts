// Built-in transport that echoes LogLayer entries to a console-shaped
// sink. The sink is **injected** at construction so the package stays
// platform-agnostic — the host passes whatever console object it wants
// (the original pre-install console on RN, a Metro-friendly console on
// Storybook, a stderr logger in Node, etc.).
//
// Echoes are BUFFERED (flushed ≈1s by default) so a chatty frame never
// hammers the JS thread with synchronous writes. The buffer registers
// itself for crash-path draining so the last Metro lines aren't lost.
//
// Unlike `LogStoreTransport`, this one is NOT a pipeline — it's a
// synchronous echo with no stages. Each entry → one console call.

import type { LogLayerTransportParams } from '@loglayer/transport';
import { LoggerlessTransport } from '@loglayer/transport';

import { createBufferedSink, type BufferedSink } from '../../primitives/index.js';
import { toStoreLevel } from '../levelMap.js';

const DEFAULT_FLUSH_MS = 1000;
const DEFAULT_MAX_BUFFER = 100;

/** Minimal console contract — every standard runtime exposes these. */
export type ConsoleSink = {
  debug?: (...args: unknown[]) => void;
  log?: (...args: unknown[]) => void;
  info?: (...args: unknown[]) => void;
  warn?: (...args: unknown[]) => void;
  error?: (...args: unknown[]) => void;
};

export type ConsoleEchoTransportOptions = {
  id?: string;
  accepts?: (event: unknown) => boolean;
  /** Where to write the echoes. Falls back to the method-per-level lookup
   *  below; a missing method is replaced by `console.log` so a partial
   *  console (e.g. one without `debug`) still works. */
  console?: ConsoleSink;
  flushMs?: number;
  maxBuffer?: number;
};

type ConsoleLine = { level: string; args: unknown[] };

export class ConsoleEchoTransport extends LoggerlessTransport {
  private readonly sink: BufferedSink<ConsoleLine>;
  private readonly console: ConsoleSink;
  private readonly accepts?: ConsoleEchoTransportOptions['accepts'];

  constructor(opts: ConsoleEchoTransportOptions = {}) {
    super({ id: opts.id ?? 'console' });
    this.accepts = opts.accepts;
    this.console = opts.console ?? {};
    this.sink = createBufferedSink<ConsoleLine>({
      flushMs: opts.flushMs ?? DEFAULT_FLUSH_MS,
      maxBuffer: opts.maxBuffer ?? DEFAULT_MAX_BUFFER,
      register: true, // drain on crash so the last lines aren't lost
      onFlush: (batch) => {
        for (const { level, args } of batch) {
          const fn =
            (
              this.console as Record<
                string,
                ((...a: unknown[]) => void) | undefined
              >
            )[level] ?? this.console.log;
          if (fn) fn(...args);
        }
      },
    });
  }

  shipToLogger({
    logLevel,
    messages,
    data,
    hasData,
  }: LogLayerTransportParams): unknown[] {
    if (this.accepts && !this.accepts(messages[0])) return messages;
    const args = hasData && data ? [...messages, data] : messages;
    this.sink.push({ level: toStoreLevel(logLevel), args });
    return messages;
  }
}
