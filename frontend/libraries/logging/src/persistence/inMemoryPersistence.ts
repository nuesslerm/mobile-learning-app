// In-memory LogPersistence implementation — the default fallback used when
// a host doesn't supply a durable backend (Storybook, unit tests, ephemeral
// Node hosts). It implements the full `LogPersistence` contract using a
// plain in-memory ring buffer so any consumer of the contract can rely on
// it working out of the box.
//
// Hosts that need durable storage (mobile app → expo-sqlite/idb, server →
// filesystem) implement their own adapter and pass it via the wiring
// point at boot.

import type {
  LogPersistence,
  PersistedLogEntry,
  QueryPageOptions,
  StoreLine,
} from './contract.js';
import { RETENTION_MAX_ROWS } from './contract.js';

type MemoryEntry = {
  dbId: number;
  ts: number;
  level: StoreLine['level'];
  text: string;
  // `| undefined` (not just `?`) because appendBatch below assigns these
  // explicitly from an optional source field — under a consumer's
  // `exactOptionalPropertyTypes: true`, a bare `?`
  // rejects an explicit `undefined` assignment even though the key is present.
  source?: string | undefined;
  context?: Record<string, unknown> | undefined;
};

export function createInMemoryPersistence(): LogPersistence {
  let nextDbId = 1;
  let buffer: MemoryEntry[] = [];

  return {
    async appendBatch(entries) {
      if (entries.length === 0) return;
      const ts = Date.now();
      const built: MemoryEntry[] = entries.map((e) => ({
        dbId: nextDbId++,
        ts,
        level: e.level,
        text: e.text,
        source: e.source,
        context:
          e.context && Object.keys(e.context).length > 0
            ? e.context
            : undefined,
      }));
      buffer = [...buffer, ...built];
      // Trim to retention. Newest-first ordering means the tail is at the end.
      if (buffer.length > RETENTION_MAX_ROWS) {
        buffer = buffer.slice(buffer.length - RETENTION_MAX_ROWS);
      }
    },

    async queryPage({
      beforeId,
      limit,
    }: QueryPageOptions): Promise<PersistedLogEntry[]> {
      // Newest-first iteration: walk the buffer from the end.
      let startIdx = buffer.length;
      if (beforeId !== undefined) {
        const found = buffer.findIndex((e) => e.dbId === beforeId);
        if (found === -1) {
          // beforeId not in the buffer — return empty (caller can retry with a
          // larger beforeId from the next snapshot).
          return [];
        }
        startIdx = found;
      }
      const slice = buffer.slice(Math.max(0, startIdx - limit), startIdx);
      return slice.reverse();
    },

    async clear() {
      buffer = [];
    },
  };
}
