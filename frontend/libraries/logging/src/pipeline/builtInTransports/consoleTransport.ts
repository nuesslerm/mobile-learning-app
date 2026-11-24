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
  /** Where to write the echoes. Falls back to the method-per-level lookup
   *  below; a missing method is replaced by `console.log` so a partial
   *  console (e.g. one without `debug`) still works. */
  console?: ConsoleSink;
  flushMs?: number;
  maxBuffer?: number;
};

type ConsoleLine = { level: ReturnType<typeof toStoreLevel>; args: unknown[] };

export class ConsoleEchoTransport extends LoggerlessTransport {
  private readonly sink: BufferedSink<ConsoleLine>;
  private readonly console: ConsoleSink;

  constructor(opts: ConsoleEchoTransportOptions = {}) {
    super({ id: 'console' });
    this.console = opts.console ?? {};
    this.sink = createBufferedSink<ConsoleLine>({
      flushMs: opts.flushMs ?? DEFAULT_FLUSH_MS,
      maxBuffer: opts.maxBuffer ?? DEFAULT_MAX_BUFFER,
      register: true, // drain on crash so the last lines aren't lost
      onFlush: (batch) => {
        for (const { level, args } of batch) {
          const fn = this.console[level] ?? this.console.log;
          fn?.apply(this.console, args);
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
    const args = hasData && data ? [...messages, data] : messages;
    this.sink.push({ level: toStoreLevel(logLevel), args });
    return messages;
  }
}
