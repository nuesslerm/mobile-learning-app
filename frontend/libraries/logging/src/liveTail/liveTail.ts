// In-memory log tail — the bounded, snapshot-based pub-sub buffer that
// drives the on-screen overlay UI.
//
// Two distinct roles from `persistence/` (which lives at a different
// layer):
//   - liveTail is FAST (sync, microsecond), EPHEMERAL (lost on restart),
//     bounded (rolling 10K window). Used for "what just happened" overlays.
//   - persistence is SLOW (async, disk I/O), DURABLE (survives restart),
//     larger (50K+ rows). Used for "load older" history queries.
//
// Snapshot contract (REQUIRED for `useSyncExternalStore`):
//   - `getEntries()` returns a NEW readonly array on every mutation so
//     React's `Object.is` snapshot comparison fails the way it should
//     and the consumer re-renders. Stale-reference readers (mutate in
//     place + emit) silently bail and the overlay stops re-rendering.
//   - Listeners receive `(snapshot, prev)` so consumers can compute the
//     delta in one pass without calling `getEntries()` again.
//   - Coalesced: a burst of `add` / `appendBatch` calls within a microtask
//     collapses into a single `emit`.
//   - A monotonic `version` counter on every mutation — stable dedup key
//     for the remote forwarder.

import type { Clock } from '../primitives/clock.js';
import { systemClock } from '../primitives/clock.js';

export type LogLevel = 'debug' | 'log' | 'info' | 'warn' | 'error';

export type LogEntry = {
  id: number;
  ts: number;
  level: LogLevel;
  text: string;
  /**
   * Optional structured fields. `source` identifies the authoring module
   * (e.g. `'EditorInput'`); `context` is the queryable metadata bag that
   * LogLayer assembles. Both are absent for raw console captures.
   */
  // `| undefined` (not just `?`) so a consumer with
  // `exactOptionalPropertyTypes: true` can assign
  // these from an explicitly-optional source without a type error.
  source?: string | undefined;
  context?: Record<string, unknown> | undefined;
};

export type AddOptions = {
  source?: string | undefined;
  context?: Record<string, unknown> | undefined;
};

/** In-memory live-tail cap. FlatList virtualizes rendering, so this is
 *  bounded by memory, not render cost. History beyond this lives in
 *  the host's persistence backend. */
export const MAX_LIVE_ENTRIES = 10000;

const EMPTY: readonly LogEntry[] = Object.freeze([]);

let snapshot: readonly LogEntry[] = EMPTY;
// `nextId` is intentionally NOT reset by a rollover (we want ids to stay
// monotonic across the session so duplicate-key detection on the FlatList
// stays sound). Only `clear()` resets it.
let nextId = 1;
let version = 0;
const listeners = new Set<LiveTailListener>();

export type LiveTailListener = (
  snapshot: readonly LogEntry[],
  prev: readonly LogEntry[]
) => void;

export function safeJson(value: unknown): string {
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

const EMIT_THROTTLE_MS = 1000;

let emitScheduled = false;
let prevSnapshotForEmit: readonly LogEntry[] = snapshot;
let lastEmitAt = -Infinity;

function doEmit(): void {
  emitScheduled = false;
  const prev = prevSnapshotForEmit;
  if (snapshot === prev) return; // no-op burst
  prevSnapshotForEmit = snapshot;
  lastEmitAt = clock.now();
  for (const l of listeners) {
    try {
      l(snapshot, prev);
    } catch {
      // never let a listener break logging
    }
  }
}

// Notifies listeners at most once per EMIT_THROTTLE_MS, no matter how many
// times add()/appendBatch()/clear() is called in that window, or which call
// site is doing the calling. The snapshot array itself is always updated
// synchronously — only this "tell listeners" step is throttled, so
// getEntries() is never stale.
function scheduleEmit(): void {
  if (emitScheduled) return;
  emitScheduled = true;
  const elapsed = clock.now() - lastEmitAt;
  if (elapsed >= EMIT_THROTTLE_MS) {
    setTimeout(doEmit, 0);
  } else {
    setTimeout(doEmit, EMIT_THROTTLE_MS - elapsed);
  }
}

function capped(next: readonly LogEntry[]): readonly LogEntry[] {
  return next.length > MAX_LIVE_ENTRIES
    ? Object.freeze(next.slice(next.length - MAX_LIVE_ENTRIES))
    : Object.freeze(next);
}

let clock: Clock = systemClock;

/** Test-only: inject a mockable clock. Production code uses
 *  `systemClock` by default. */
export function __setLiveTailClock(c: Clock): void {
  clock = c;
}

export const liveTail = {
  add(level: LogLevel, text: string, opts?: AddOptions): LogEntry {
    const entry: LogEntry = {
      id: nextId++,
      ts: clock.now(),
      level,
      text,
      source: opts?.source,
      context:
        opts?.context && Object.keys(opts.context).length > 0
          ? opts.context
          : undefined,
    };
    const next: readonly LogEntry[] = capped(
      snapshot.length === 0 ? [entry] : [...snapshot, entry]
    );
    snapshot = next;
    version++;
    scheduleEmit();
    return entry;
  },

  appendBatch(
    entries: readonly {
      level: LogLevel;
      text: string;
      source?: string | undefined;
      context?: Record<string, unknown> | undefined;
    }[]
  ): LogEntry[] {
    if (entries.length === 0) return [];
    const ts = clock.now();
    const built: LogEntry[] = entries.map((e) => ({
      id: nextId++,
      ts,
      level: e.level,
      text: e.text,
      source: e.source,
      context:
        e.context && Object.keys(e.context).length > 0 ? e.context : undefined,
    }));
    const nextBatch: readonly LogEntry[] = capped(
      snapshot.length === 0 ? built : [...snapshot, ...built]
    );
    snapshot = nextBatch;
    version++;
    scheduleEmit();
    return built;
  },

  getEntries(): readonly LogEntry[] {
    return snapshot;
  },

  /** Monotonic mutation counter. Stable dedup key for the remote forwarder. */
  getVersion(): number {
    return version;
  },

  clear(): void {
    snapshot = EMPTY;
    nextId = 1;
    version++;
    lastEmitAt = -Infinity;
    scheduleEmit();
  },

  subscribe(listener: LiveTailListener): () => void {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  },

  formatAll(): string {
    return snapshot
      .map((e) => {
        const src = e.source ? ` [${e.source}]` : '';
        const ctx = e.context ? ` ${safeJson(e.context)}` : '';
        return `${new Date(e.ts).toISOString()} [${e.level.toUpperCase()}]${src} ${e.text}${ctx}`;
      })
      .join('\n');
  },
};

/** Test-only: reset the live tail to its initial state. */
export function __resetLiveTail(): void {
  snapshot = EMPTY;
  nextId = 1;
  version = 0;
  listeners.clear();
  emitScheduled = false;
  prevSnapshotForEmit = EMPTY;
  lastEmitAt = -Infinity;
  clock = systemClock;
}
