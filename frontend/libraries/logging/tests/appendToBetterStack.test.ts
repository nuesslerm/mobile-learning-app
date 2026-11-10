import type { LogLayerTransportParams } from '@loglayer/transport';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { __resetLiveTail, liveTail } from '../src/liveTail';
import {
  PipelineTransport,
  appendToBetterStack,
  type BetterStackFetch,
} from '../src/pipeline';
import { flushAllBufferedSinks } from '../src/primitives';

type Entry = { level: string; text: string; source?: string; context?: Record<string, unknown>; ts?: number };
type Post = { url: string; init: Parameters<BetterStackFetch>[1] };

const HOST = 'ingest.example.test';
const TOKEN = 'unit-test-ingest-token';

function recordingFetch(respond: () => Promise<{ ok: boolean; status: number }> = () => Promise.resolve({ ok: true, status: 202 })) {
  const posts: Post[] = [];
  const fetch: BetterStackFetch = (url, init) => {
    posts.push({ url, init });
    return respond();
  };
  const items = () => posts.flatMap((p) => JSON.parse(p.init.body) as Record<string, unknown>[]);
  return { posts, fetch, items };
}

const failures = () => liveTail.getEntries().filter((e) => e.text === 'logging.forward_failed');

beforeEach(() => {
  vi.useFakeTimers();
  __resetLiveTail();
});

afterEach(() => {
  flushAllBufferedSinks();
  vi.useRealTimers();
});

describe('appendToBetterStack', () => {
  it('given entries staged, when the cadence passes, then one POST to the host carries them with the bearer token and fixed fields', () => {
    // Given a stage with a recording fetch and fixed fields
    const net = recordingFetch();
    const stage = appendToBetterStack({ host: HOST, token: TOKEN, fetch: net.fetch, fields: { app: 'demo', platform: 'ios' } });

    // When two entries are staged, before and after the flush cadence
    stage([
      { level: 'info', text: 'app.boot', source: 'AppShell', ts: Date.UTC(2026, 9, 22, 8, 0, 0) },
      { level: 'error', text: 'app.crash', context: { kind: 'error', fatal: true } },
    ] satisfies Entry[]);
    const beforeCadence = net.posts.length;
    vi.advanceTimersByTime(2000);

    // Then nothing went out before the cadence, one POST carries both, and the token rides only in its header
    expect(beforeCadence).toBe(0);
    expect(net.posts).toHaveLength(1);
    const [post] = net.posts;
    expect(post?.url).toBe(`https://${HOST}`);
    expect(post?.init).toMatchObject({
      method: 'POST',
      keepalive: true,
      headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${TOKEN}` },
    });
    expect(net.items()).toEqual([
      { app: 'demo', platform: 'ios', dt: '2026-10-22T08:00:00.000Z', level: 'info', message: 'app.boot', source: 'AppShell' },
      expect.objectContaining({ app: 'demo', platform: 'ios', level: 'error', message: 'app.crash', kind: 'error', fatal: true }),
    ]);
    expect(post?.init.body).not.toContain(TOKEN);
  });

  it('given context keys that clash with the item fields, when posted, then the item fields win and the context values are kept beside them', () => {
    // Given a crash whose context carries its own message, and a field the fixed fields also set
    const net = recordingFetch();
    const stage = appendToBetterStack({ host: HOST, token: TOKEN, fetch: net.fetch, fields: { app: 'demo' } });

    // When it is staged and drained
    stage([{ level: 'error', text: 'app.crash', context: { message: 'boom', app: 'other', level: 'x' } }]);
    flushAllBufferedSinks();

    // Then the event name, level and app stay, and the clashing values move under context_*
    expect(net.items()[0]).toMatchObject({
      message: 'app.crash',
      level: 'error',
      app: 'demo',
      context_message: 'boom',
      context_app: 'other',
      context_level: 'x',
    });
  });

  it('given the stage behind a PipelineTransport, when the crash drain runs with lines still in the pipeline buffer, then they are posted at once', () => {
    // Given a pipeline whose stage has already seen one batch, and a crash line still buffered
    const net = recordingFetch();
    const transport = new PipelineTransport<Entry>({
      serialize: (p: LogLayerTransportParams) => ({ level: p.logLevel, text: String(p.messages[0]) }),
      stages: [appendToBetterStack({ host: HOST, token: TOKEN, fetch: net.fetch })],
    });
    const ship = (level: LogLayerTransportParams['logLevel'], text: string) =>
      transport.shipToLogger({ logLevel: level, messages: [text], hasData: false } as LogLayerTransportParams);
    ship('info', 'app.boot');
    vi.advanceTimersByTime(1000);
    ship('error', 'app.crash');

    // When the global handler drains every buffered sink
    flushAllBufferedSinks();

    // Then both lines are posted without waiting for any timer
    expect(net.items().map((i) => i.message)).toEqual(['app.boot', 'app.crash']);
  });

  it('given a sink that fails, when two batches are posted, then each is dropped without a retry and the live tail gets one forward_failed line', async () => {
    // Given an ingest that answers 401, then rejects
    const answers = [
      () => Promise.resolve({ ok: false, status: 401 }),
      () => Promise.reject(new Error('network down')),
    ];
    let calls = 0;
    const net = recordingFetch(() => answers[calls++]!());
    const stage = appendToBetterStack({ host: HOST, token: TOKEN, fetch: net.fetch });

    // When two batches are posted, a cadence apart
    stage([{ level: 'info', text: 'first' }]);
    flushAllBufferedSinks();
    await vi.runAllTimersAsync();
    stage([{ level: 'info', text: 'second' }]);
    flushAllBufferedSinks();
    await vi.runAllTimersAsync();

    // Then each batch went out once, nothing was resent, and one failure line names the status, never the token
    expect(net.items().map((i) => i.message)).toEqual(['first', 'second']);
    expect(failures()).toHaveLength(1);
    expect(failures()[0]?.context).toMatchObject({ status: 401, dropped: 1, url: `https://${HOST}` });
    expect(JSON.stringify(liveTail.getEntries())).not.toContain(TOKEN);
  });

  it('given a fetch that throws and a context that cannot be serialized, when posted, then nothing throws and the line still goes out', () => {
    // Given one fetch that throws synchronously, and an entry whose context is circular
    const circular: Record<string, unknown> = { name: 'loop' };
    circular.self = circular;
    const throwing = appendToBetterStack({
      host: HOST,
      token: TOKEN,
      fetch: () => {
        throw new Error('bad url');
      },
    });
    const net = recordingFetch();
    const stage = appendToBetterStack({ host: `https://${HOST}/`, token: TOKEN, fetch: net.fetch });

    // When both are staged and drained
    expect(() => {
      throwing([{ level: 'info', text: 'lost' }]);
      stage([{ level: 'warn', text: 'odd', context: { circular } }]);
      flushAllBufferedSinks();
    }).not.toThrow();

    // Then the throw became one failure line, and the circular entry kept its line without its fields
    expect(failures().map((e) => e.context?.reason)).toEqual(['bad url']);
    expect(net.posts[0]?.url).toBe(`https://${HOST}`);
    expect(net.items()).toEqual([expect.objectContaining({ level: 'warn', message: 'odd', unserializable: true })]);
  });

  it('given a batch over the keepalive budget, when posted, then it is sent without keepalive so the browser does not reject it', () => {
    // Given a stage and a batch of long lines
    const net = recordingFetch();
    const stage = appendToBetterStack({ host: HOST, token: TOKEN, fetch: net.fetch });

    // When it is drained
    stage(Array.from({ length: 20 }, () => ({ level: 'info', text: 'x'.repeat(1000) })));
    flushAllBufferedSinks();

    // Then the request is a plain one
    expect(net.posts[0]?.init.keepalive).toBe(false);
  });

  it('given a keepalive post still in flight, when the next one would push the in-flight total over the budget, then it goes without keepalive until the first settles', async () => {
    // Given an ingest that holds its first answer
    let releaseFirst: () => void = () => undefined;
    const answers = [
      () => new Promise<{ ok: boolean; status: number }>((resolve) => (releaseFirst = () => resolve({ ok: true, status: 202 }))),
      () => Promise.resolve({ ok: true, status: 202 }),
      () => Promise.resolve({ ok: true, status: 202 }),
    ];
    let calls = 0;
    const net = recordingFetch(() => answers[calls++]!());
    const stage = appendToBetterStack({ host: HOST, token: TOKEN, fetch: net.fetch });
    const postLong = () => {
      stage([{ level: 'info', text: 'x'.repeat(10_000) }]);
      flushAllBufferedSinks();
    };

    // When two 10 KB batches go out while the first is unanswered, then a third after it is answered
    postLong();
    postLong();
    releaseFirst();
    await vi.runAllTimersAsync();
    postLong();

    // Then only the second, which would have overlapped the first, gave up keepalive
    expect(net.posts.map((p) => p.init.keepalive)).toEqual([true, false, true]);
  });

  it('given a crash during boot, when the drain runs before the pipeline ever flushed, then app.boot and app.crash are posted at once', () => {
    // Given a pipeline whose stage has seen no batch yet, so its own sink does not exist
    const net = recordingFetch();
    const transport = new PipelineTransport<Entry>({
      serialize: (p: LogLayerTransportParams) => ({ level: p.logLevel, text: String(p.messages[0]) }),
      stages: [appendToBetterStack({ host: HOST, token: TOKEN, fetch: net.fetch })],
    });
    transport.shipToLogger({ logLevel: 'info', messages: ['app.boot'], hasData: false } as LogLayerTransportParams);
    transport.shipToLogger({ logLevel: 'error', messages: ['app.crash'], hasData: false } as LogLayerTransportParams);

    // When the global handler drains every buffered sink, with no timer advanced
    flushAllBufferedSinks();

    // Then both lines went out in that drain: the stage's sink, made during it, was drained too
    expect(net.items().map((i) => i.message)).toEqual(['app.boot', 'app.crash']);
  });

  it('given an entry whose ts is outside the Date range, when posted, then it gets the current time and the rest of the batch still goes out', () => {
    // Given a stage and a batch whose first entry carries an impossible time
    const net = recordingFetch();
    const stage = appendToBetterStack({ host: HOST, token: TOKEN, fetch: net.fetch });

    // When it is staged and drained
    expect(() =>
      stage([
        { level: 'info', text: 'far', ts: 9e15 },
        { level: 'info', text: 'next' },
      ])
    ).not.toThrow();
    flushAllBufferedSinks();

    // Then both items are posted with a valid time
    const items = net.items();
    expect(items.map((i) => i.message)).toEqual(['far', 'next']);
    expect(Number.isNaN(Date.parse(String(items[0]?.dt)))).toBe(false);
  });

  it('given minLevel info, when every level is logged, then debug stays local and the rest is posted', () => {
    // Given a stage that forwards info and up
    const net = recordingFetch();
    const stage = appendToBetterStack({ host: HOST, token: TOKEN, fetch: net.fetch, minLevel: 'info' });

    // When one entry per level is staged and drained
    stage(['debug', 'info', 'warn', 'error'].map((level) => ({ level, text: `at.${level}` })));
    flushAllBufferedSinks();

    // Then the debug line was not posted
    expect(net.items().map((i) => i.message)).toEqual(['at.info', 'at.warn', 'at.error']);
  });

  it('given an outage that ends, when a later post fails again, then that second outage gets its own forward_failed line', async () => {
    // Given an ingest that fails, recovers, then fails again
    const answers = [
      () => Promise.resolve({ ok: false, status: 503 }),
      () => Promise.resolve({ ok: false, status: 503 }),
      () => Promise.resolve({ ok: true, status: 202 }),
      () => Promise.resolve({ ok: false, status: 429 }),
    ];
    let calls = 0;
    const net = recordingFetch(() => answers[calls++]!());
    const stage = appendToBetterStack({ host: HOST, token: TOKEN, fetch: net.fetch });

    // When four batches are posted one after another
    for (const text of ['a', 'b', 'c', 'd']) {
      stage([{ level: 'info', text }]);
      flushAllBufferedSinks();
      await vi.runAllTimersAsync();
    }

    // Then each outage was reported once
    expect(failures().map((e) => e.context?.status)).toEqual([503, 429]);
  });
});
