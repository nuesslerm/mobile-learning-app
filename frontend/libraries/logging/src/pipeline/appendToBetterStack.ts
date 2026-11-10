// The failure line goes straight to the live tail, never through the pipeline, so a dead sink cannot feed itself.
// Entries arrive scrubbed: `configureLogger` puts the scrubber on the root LogLayer, ahead of every transport.

import { liveTail } from '../liveTail/liveTail.js';
import { createBufferedSink, type BufferedSink } from '../primitives/bufferedSink.js';

type BetterStackEntry = {
  level: string;
  text: string;
  source?: string | undefined;
  context?: Record<string, unknown> | undefined;
  ts?: number | undefined;
};

type FetchInit = {
  method: 'POST';
  headers: Record<string, string>;
  body: string;
  keepalive: boolean;
};

export type BetterStackFetch = (url: string, init: FetchInit) => Promise<{ ok: boolean; status: number }>;

export type BetterStackLevel = 'debug' | 'info' | 'warn' | 'error';

export type AppendToBetterStackOptions = {
  /** Ingest host; each flush posts one JSON array to `https://<host>`. */
  host: string;
  /** Source token, sent as the bearer token. */
  token: string;
  /** Fixed fields on every item. A context key that clashes with them or the core fields is kept as `context_<key>`. */
  fields?: Record<string, unknown>;
  /** Entries below this level stay local. Default 'debug' (all). */
  minLevel?: BetterStackLevel;
  /** Default: `globalThis.fetch`, read once at creation. */
  fetch?: BetterStackFetch;
  /** Flush cadence in ms. Default 2000. */
  flushMs?: number;
  /** A window producing this many entries posts at once. Default 100. */
  maxBuffer?: number;
};

type Item = Record<string, unknown>;

const DEFAULT_FLUSH_MS = 2000;
const DEFAULT_MAX_BUFFER = 100;
// browsers cap the bodies of all in-flight keepalive requests at 64 KiB together; a UTF-16 unit is at most 3 UTF-8 bytes
const KEEPALIVE_BUDGET_CHARS = 16_384;
const MAX_DATE_MS = 8.64e15;
const CORE_KEYS = ['dt', 'level', 'message', 'source'];
const LEVEL_RANK: Record<string, number> = { debug: 0, info: 1, warn: 2, error: 3 };

// typed sync, not Stage's `void | Promise<void>`: nothing on the log path waits for the network
export function appendToBetterStack(opts: AppendToBetterStackOptions): (batch: readonly BetterStackEntry[]) => void {
  const url = `https://${opts.host.replace(/^https?:\/\//, '').replace(/\/+$/, '')}`;
  const fields = opts.fields ?? {};
  const reserved = new Set([...CORE_KEYS, ...Object.keys(fields)]);
  const minRank = LEVEL_RANK[opts.minLevel ?? 'debug'] ?? 0;
  const send = opts.fetch ?? defaultFetch();
  let keepaliveInFlight = 0;
  let outage = false;
  let sink: BufferedSink<Item> | undefined;

  // one line per outage: a success ends it, so the next failure is reported again
  function failed(detail: Record<string, unknown>): void {
    if (outage) return;
    outage = true;
    liveTail.add('warn', 'logging.forward_failed', { source: 'appendToBetterStack', context: { url, ...detail } });
  }

  function post(batch: Item[]): Promise<void> | undefined {
    if (!send) return undefined;
    const body = encode(batch);
    const keepalive = keepaliveInFlight + body.length <= KEEPALIVE_BUDGET_CHARS;
    if (keepalive) keepaliveInFlight += body.length;
    const settle = () => {
      if (keepalive) keepaliveInFlight -= body.length;
    };
    try {
      return send(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${opts.token}` },
        body,
        keepalive,
      }).then(
        (res) => {
          settle();
          if (res.ok) outage = false;
          else failed({ status: res.status, dropped: batch.length });
        },
        (err: unknown) => {
          settle();
          failed({ reason: reasonOf(err), dropped: batch.length });
        }
      );
    } catch (err) {
      settle();
      failed({ reason: reasonOf(err), dropped: batch.length });
      return undefined;
    }
  }

  function toItem(entry: BetterStackEntry): Item {
    const item: Item = {};
    for (const [key, value] of Object.entries(entry.context ?? {})) {
      item[reserved.has(key) ? `context_${key}` : key] = value;
    }
    Object.assign(item, fields);
    // a ts outside Date's range would make toISOString throw
    const ts = typeof entry.ts === 'number' && Math.abs(entry.ts) <= MAX_DATE_MS ? entry.ts : Date.now();
    item.dt = new Date(ts).toISOString();
    item.level = entry.level;
    item.message = entry.text;
    if (entry.source !== undefined) item.source = entry.source;
    return item;
  }

  return (batch) => {
    if (!send || batch.length === 0) return;
    // created on the first batch, so it registers after the pipeline's own sink: a crash drain walks the
    // registry in order, and the pipeline hands this stage its last lines before this sink is drained
    sink ??= createBufferedSink<Item>({
      flushMs: opts.flushMs ?? DEFAULT_FLUSH_MS,
      maxBuffer: opts.maxBuffer ?? DEFAULT_MAX_BUFFER,
      register: true,
      onFlush: post,
    });
    for (const entry of batch) {
      if (typeof entry?.text !== 'string' || typeof entry?.level !== 'string') continue;
      if ((LEVEL_RANK[entry.level] ?? minRank) < minRank) continue;
      try {
        sink.push(toItem(entry));
      } catch {
        // logging must never throw; one bad entry must not cost the rest of the batch
      }
    }
  };
}

function defaultFetch(): BetterStackFetch | undefined {
  const fetch = (globalThis as { fetch?: BetterStackFetch }).fetch;
  return typeof fetch === 'function' ? fetch.bind(globalThis) : undefined;
}

// one item that cannot be serialized keeps its line and loses only its fields
function encode(batch: Item[]): string {
  const parts = batch.map((item) => {
    try {
      return JSON.stringify(item, jsonSafe);
    } catch {
      const { dt, level, message, source } = item;
      return JSON.stringify({ dt, level, message, source, unserializable: true });
    }
  });
  return `[${parts.join(',')}]`;
}

function jsonSafe(_key: string, value: unknown): unknown {
  if (value instanceof Error) return { name: value.name, message: value.message, stack: value.stack };
  if (typeof value === 'bigint') return value.toString();
  return value;
}

function reasonOf(err: unknown): string {
  if (err instanceof Error) return err.message;
  return typeof err === 'string' ? err : Object.prototype.toString.call(err);
}
