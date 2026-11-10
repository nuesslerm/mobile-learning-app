// Pipeline stage factory — appends a batch of entries to the in-memory
// live tail. Co-located with the liveTail implementation so the
// `Stage<T>` knows exactly what shape the tail expects.
//
//   import { PipelineTransport, appendToLiveTail } from '@lib/logging/pipeline';
//   import { appendToLiveTail } from '@lib/logging/liveTail';
//
//   const stages = [
//     appendToLiveTail(),
//     appendToPersistence(persistence),
//   ];
//   new PipelineTransport({ serialize, stages });

import type { Stage } from '../pipeline/stage.js';

import { liveTail, type LogLevel } from './liveTail.js';

type AppendToLiveTailInput = {
  level: LogLevel;
  text: string;
  source?: string;
  context?: Record<string, unknown>;
};

/**
 * Stage that appends every batch to `liveTail`. Drops entries with
 * missing required fields rather than throwing (a stage must never
 * crash the pipeline).
 */
export function appendToLiveTail(): Stage<AppendToLiveTailInput> {
  return (batch) => {
    if (batch.length === 0) return;
    // Filter to the subset the tail actually accepts (it requires `level`
    // and `text` as the minimum entry shape).
    const lines: {
      level: LogLevel;
      text: string;
      source?: string;
      context?: Record<string, unknown>;
    }[] = [];
    for (const e of batch) {
      if (typeof e?.text === 'string' && typeof e?.level === 'string') {
        lines.push(e);
      }
    }
    if (lines.length === 0) return;
    liveTail.appendBatch(lines);
  };
}
