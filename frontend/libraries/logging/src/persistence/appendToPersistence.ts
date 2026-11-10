// Pipeline stage factory — appends a batch of entries to the host's
// durable `LogPersistence` adapter (expo-sqlite / idb / etc).
// Co-located with the persistence contract so the stage knows exactly
// what shape the adapter expects.
//
// Fire-and-forget: a slow SQLite write must never block the pipeline.
// Errors are swallowed (the persistence adapter itself logs+swallows).

import type { LogLevel } from '../liveTail/index.js';
import type { Stage } from '../pipeline/stage.js';

import type { LogPersistence, StoreLine } from './contract.js';

type AppendToPersistenceInput = {
  level: LogLevel;
  text: string;
  source?: string;
  context?: Record<string, unknown>;
};

/** Stage that appends every batch to a host-supplied `LogPersistence`. */
export function appendToPersistence(
  persistence: LogPersistence
): Stage<AppendToPersistenceInput> {
  return (batch) => {
    if (batch.length === 0) return;
    const lines: StoreLine[] = batch.map((e) => ({
      level: e.level,
      text: e.text,
      source: e.source,
      context: e.context ?? {},
    }));
    void persistence.appendBatch(lines).catch(() => undefined);
  };
}
