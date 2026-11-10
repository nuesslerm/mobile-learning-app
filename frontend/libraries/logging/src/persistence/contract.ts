// LogPersistence — the contract between the package and any host-provided
// durable log store.
//
// The package ships a generic buffering/transports/forwarder architecture.
// Durable storage behind it is host-specific (expo-sqlite on native, idb
// on web, files on Node, in-memory in tests). Hosts implement this
// interface; the package's transports/forwarders consume it without
// knowing the backend.
//
// All methods MUST degrade silently on init failure — a persistence
// hiccup must never break the live in-memory tail. Backends log + swallow
// their own init errors and return safe no-ops.
//
// `appendBatch` is fire-and-forget from the transport's perspective:
// callers do `void persistence.appendBatch(...)` so a slow SQLite write
// never blocks the logging pipeline.

export type PersistedLogEntry = {
  /** Backend-assigned stable id (SQLite ROWID / IDB autoIncrement). Used by
   *  `queryPage({ beforeId })` to page older entries. */
  dbId: number;
  ts: number;
  level: import('../liveTail/index.js').LogLevel;
  text: string;
  // `| undefined` (not just `?`) so a consumer with
  // `exactOptionalPropertyTypes: true` can assign
  // these from an explicitly-optional source without a type error.
  source?: string | undefined;
  context?: Record<string, unknown> | undefined;
};

/** Inputs accepted by `appendBatch` — entries to be persisted in bulk. */
export type StoreLine = {
  level: import('../liveTail/index.js').LogLevel;
  text: string;
  source?: string | undefined;
  context?: Record<string, unknown> | undefined;
};

export type QueryPageOptions = {
  /** Return entries strictly older than this dbId. Omit for the newest page. */
  beforeId?: number;
  limit: number;
};

/**
 * Durable log storage. Backends expose append + page-query + clear.
 *
 * Conventions:
 * - `appendBatch` MUST accept an empty `entries` array as a no-op.
 * - `queryPage` MUST return newest-first within the page; an empty array
 *   when there's nothing to return.
 * - `clear` MUST reset the entire store.
 *
 * The package never awaits `appendBatch` from the hot path — fire-and-
 * forget is the contract. `queryPage` and `clear` are user-driven.
 */
export interface LogPersistence {
  appendBatch(entries: readonly StoreLine[]): Promise<void>;
  /** Newest-first within the returned page. */
  queryPage(opts: QueryPageOptions): Promise<PersistedLogEntry[]>;
  clear(): Promise<void>;
}

/** Suggested cap for the durable store. The in-memory live tail caps at
 *  `MAX_LIVE_ENTRIES` (10000); history beyond that goes to the host's
 *  persistence backend. 50000 = ~5x the live tail, ~1MB on disk. */
export const RETENTION_MAX_ROWS = 50000;
