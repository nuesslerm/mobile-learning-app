// Shared buffering primitive for the logging pipeline. Every sink in the
// pipeline is buffered BY DEFAULT (console echo, the logStore feeder,
// hot-path wide events, the remote outbox) so logging never taxes the
// frame: items are queued in memory and flushed in batches on a fixed
// cadence (+ a size cap that drains a flood immediately).
//
// This is the local half of "log as much as possible, safely" — any
// transport-level or remote forwarder buffer lives as a `BufferedSink<T>`
// so the drain-on-crash machinery (`flushAllBufferedSinks()`) can sweep
// every queue in one pass from a global error handler.
//
// The buffer is purely JS — no platform deps. Safe to use on web, native,
// and Node hosts.

/** Set of drain callbacks for every sink that opted into crash-path
 *  draining (`register: true` at construction). Drained on demand by
 *  `flushAllBufferedSinks()` from the global error handler before the JS
 *  context tears down. Each returns its send's promise, if the send is async. */
const registry = new Set<() => Promise<void> | undefined>();

/** Drain every registered buffered sink now. Best-effort, never throws. */
export function flushAllBufferedSinks(): void {
  void drainAllBufferedSinks();
}

/**
 * Drain every registered buffered sink now, like `flushAllBufferedSinks()`, and settle once every send the
 * drain started has settled, or after `timeoutMs`. Never rejects.
 */
export function drainAllBufferedSinks(timeoutMs?: number): Promise<void> {
  const sends: Promise<void>[] = [];
  // a sink registered during the drain (a stage's own, made on its first batch) is drained too: a Set's
  // iteration visits entries added while it runs
  for (const drain of registry) {
    try {
      const sent = drain();
      if (sent) sends.push(sent);
    } catch {
      // logging must never throw
    }
  }
  if (sends.length === 0) return Promise.resolve();
  const settled = Promise.allSettled(sends).then(() => undefined);
  if (timeoutMs === undefined) return settled;
  return new Promise<void>((resolve) => {
    const timer = setTimeout(resolve, timeoutMs);
    void settled.then(() => {
      clearTimeout(timer);
      resolve();
    });
  });
}

export type BufferedSink<T> = {
  /** Queue one item. Buffered; flushed on the interval or when the cap is hit. */
  push: (item: T) => void;
  /** Force-emit the current buffer immediately. */
  flushNow: () => void;
};

export type BufferedSinkOptions<T> = {
  /** Flush cadence in ms (the buffer drains at most this stale). */
  flushMs: number;
  /** Hard cap: a window producing this many items drains immediately. */
  maxBuffer: number;
  /** Receives the batch to ship. Must not throw (wrapped defensively anyway). */
  onFlush: (batch: T[]) => void | Promise<void>;
  /** Register with `flushAllBufferedSinks()` (crash path). Default: false. */
  register?: boolean;
};

export function createBufferedSink<T>(
  opts: BufferedSinkOptions<T>
): BufferedSink<T> {
  let buffer: T[] = [];
  let timer: ReturnType<typeof setTimeout> | null = null;

  function drain(): Promise<void> | undefined {
    if (timer != null) {
      clearTimeout(timer);
      timer = null;
    }
    if (buffer.length === 0) return undefined;
    const batch = buffer;
    buffer = [];
    try {
      const sent = opts.onFlush(batch);
      return sent
        ? Promise.resolve(sent).catch(() => {
            // logging must never throw
          })
        : undefined;
    } catch {
      // logging must never throw
      return undefined;
    }
  }

  function flushNow(): void {
    void drain();
  }

  function schedule(): void {
    if (timer != null) return;
    timer = setTimeout(() => {
      timer = null;
      flushNow();
    }, opts.flushMs);
  }

  function push(item: T): void {
    buffer.push(item);
    // Safety valve: drain immediately if a window produced a flood so the buffer
    // can't grow unbounded between ticks.
    if (buffer.length >= opts.maxBuffer) {
      flushNow();
      return;
    }
    schedule();
  }

  if (opts.register) registry.add(drain);

  return { push, flushNow };
}
