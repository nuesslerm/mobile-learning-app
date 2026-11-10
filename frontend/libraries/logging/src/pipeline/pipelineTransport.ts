// Pipeline transport — the composition primitive for the package.
//
// A LogLayer transport receives one entry per `log.info(...)` call.
// `PipelineTransport<T>` runs each entry through a chain of stages:
// in-memory live tail → durable backend → remote sink → etc.
//
// Stages are flat functions: `(batch: T[]) => Promise<void> | void`.
// The pipeline calls them sequentially per batch (so a slow persistence
// write doesn't delay the in-memory write), but the batch itself is
// reused — there's no copying between stages. The buffer is registered
// for crash-path drain (`register: true`) so the very last entries
// reach every stage before JS tears down.

import type { LogLayerTransportParams } from '@loglayer/transport';
import { LoggerlessTransport } from '@loglayer/transport';

import { createBufferedSink, type BufferedSink } from '../primitives/index.js';

import type { Stage } from './stage.js';

export type PipelineTransportOptions<T> = {
  /** Build the entry shape from LogLayer's transport params. The pipeline
   *  calls this once per `log.*` call, then accumulates the result in the
   *  buffer. */
  serialize: (params: LogLayerTransportParams) => T;
  /** Ordered list of stages. Each stage is called with the buffered
   *  batch on flush. Earlier stages run first; later stages see the same
   *  batch reference. */
  stages: readonly Stage<T>[];
  /** Flush cadence in ms (the buffer drains at most this stale). Default 1000. */
  flushMs?: number;
  /** Hard cap: a window producing this many items drains immediately. Default 100. */
  maxBuffer?: number;
};

const DEFAULT_FLUSH_MS = 1000;
const DEFAULT_MAX_BUFFER = 100;

export class PipelineTransport<T> extends LoggerlessTransport {
  private readonly opts: PipelineTransportOptions<T>;
  private readonly sink: BufferedSink<T>;

  constructor(opts: PipelineTransportOptions<T>) {
    super({
      id:
        opts.stages.length > 0
          ? `pipeline-${opts.stages.length}`
          : 'pipeline-empty',
    });
    this.opts = opts;
    this.sink = createBufferedSink<T>({
      flushMs: opts.flushMs ?? DEFAULT_FLUSH_MS,
      maxBuffer: opts.maxBuffer ?? DEFAULT_MAX_BUFFER,
      register: true, // drain on crash so the last lines reach every stage
      onFlush: (batch) => {
        // Stages run with the SAME batch reference — no copying, no
        // re-serialization. A stage that mutates the batch would corrupt
        // later stages; document as a contract that stages are read-only.
        for (const stage of opts.stages) {
          try {
            const result = stage(batch);
            // If the stage returned a thenable, catch its rejection
            // separately — we don't await here because we want stages to
            // make progress in parallel within a single batch.
            if (result && typeof result.then === 'function') {
              result.catch(() => undefined);
            }
          } catch {
            // logging must never throw
          }
        }
      },
    });
  }

  shipToLogger(params: LogLayerTransportParams): unknown[] {
    this.sink.push(this.opts.serialize(params));
    return params.messages;
  }
}
