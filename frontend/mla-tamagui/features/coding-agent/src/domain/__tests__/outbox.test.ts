// features/coding-agent/src/domain/__tests__/outbox.test.ts
//
// Tests for the pure outbox queue ops (`enqueue`, `markAttempt`, `remove`,
// `nextDue`) and the agent-URL → WS-URL helper (`toWsUrl`). Ported in spirit
// from references/t3code/apps/mobile/src/state/thread-outbox.ts and the
// mobile WS connection module: no `effect`, no IO. Every test is
// Given/When/Then; outbox items are built with the `makeItem` helper (branded
// threadId via `.parse`).

import { ThreadId } from '@app/remote-chat-agent-contracts/orchestration';
import { describe, expect, it } from 'vitest';

import {
  enqueue,
  markAttempt,
  nextDue,
  type OutboxItem,
  OutboxItemSchema,
  remove,
} from '../outbox';
import { toWsUrl } from '../wsUrl';

function makeItem(overrides: Partial<OutboxItem> = {}): OutboxItem {
  return {
    id: 'item-1',
    threadId: ThreadId.parse('thread-1'),
    command: { kind: 'ping' },
    createdAt: 1_000,
    attempts: 0,
    ...overrides,
  };
}

describe('enqueue', () => {
  it('given an empty outbox, when an item is enqueued, then the item is appended', () => {
    // Given an empty outbox
    const items: OutboxItem[] = [];
    const item = makeItem();
    // When enqueued
    const next = enqueue(items, item);
    // Then the new list contains the item
    expect(next).toEqual([item]);
  });

  it('given an outbox with one item, when a second item is enqueued, then insertion order is preserved', () => {
    // Given an outbox with item-1
    const item1 = makeItem({ id: 'item-1' });
    const item2 = makeItem({ id: 'item-2', createdAt: 2_000 });
    // When item-2 is enqueued
    const next = enqueue([item1], item2);
    // Then both items are present in insertion order
    expect(next).toEqual([item1, item2]);
  });
});

describe('markAttempt', () => {
  it('given an outbox containing the id, when marked, then only that item has its attempts incremented', () => {
    // Given item-1 with attempts=0 and item-2 with attempts=1
    const item1 = makeItem({ id: 'item-1' });
    const item2 = makeItem({ id: 'item-2', attempts: 1 });
    // When item-1 is marked
    const next = markAttempt([item1, item2], 'item-1');
    // Then item-1's attempts go from 0 to 1 and item-2 is unchanged
    expect(next).toEqual([{ ...item1, attempts: 1 }, item2]);
  });

  it('given an outbox without the id, when marked, then every item is returned unchanged', () => {
    // Given an outbox that does not contain the target id
    const item = makeItem();
    // When the missing id is marked
    const next = markAttempt([item], 'missing');
    // Then the list is structurally identical
    expect(next).toEqual([item]);
  });
});

describe('remove', () => {
  it('given an outbox containing the id, when removed, then only that item is dropped', () => {
    // Given an outbox with item-1 and item-2
    const item1 = makeItem({ id: 'item-1' });
    const item2 = makeItem({ id: 'item-2' });
    // When item-1 is removed
    const next = remove([item1, item2], 'item-1');
    // Then only item-2 remains
    expect(next).toEqual([item2]);
  });

  it('given an outbox without the id, when removed, then the list is unchanged', () => {
    // Given an outbox that does not contain the target id
    const item = makeItem();
    // When the missing id is removed
    const next = remove([item], 'missing');
    // Then the item is still there
    expect(next).toEqual([item]);
  });
});

describe('nextDue', () => {
  it('given an item with attempts=0, when the current time is at or past its createdAt, then the item is the next due', () => {
    // Given a single item queued at t=1000 with zero attempts
    const item = makeItem({ id: 'item-1', createdAt: 1_000, attempts: 0 });
    // When the drain runs at t=2000
    const next = nextDue([item], 2_000);
    // Then the same item comes back (zero backoff makes it immediately due)
    expect(next).toBe(item);
  });

  it('given a retried item still inside its backoff window, when the drain runs, then it is not yet due', () => {
    // Given a retried item queued at t=1000 with 1 attempt (default backoff 2s)
    const item = makeItem({ id: 'item-1', createdAt: 1_000, attempts: 1 });
    // When the drain runs at t=2000 (1s after enqueue, 1s before the backoff expires)
    const next = nextDue([item], 2_000);
    // Then no item is due
    expect(next).toBeUndefined();
  });

  it('given two due items, when the drain runs, then the one queued earliest is picked first', () => {
    // Given two due items, the older one queued first
    const older = makeItem({ id: 'older', createdAt: 1_000, attempts: 0 });
    const newer = makeItem({ id: 'newer', createdAt: 1_500, attempts: 0 });
    // When the drain runs well after both
    const next = nextDue([older, newer], 10_000);
    // Then the older item wins
    expect(next).toBe(older);
  });

  it('given a fresh item and a retried item still inside its backoff, when the drain runs, then the fresh item is picked', () => {
    // Given a fresh item (attempts=0, due immediately) and a retried one (attempts=1, due at createdAt+2s)
    const fresh = makeItem({ id: 'fresh', createdAt: 2_000, attempts: 0 });
    const retried = makeItem({ id: 'retried', createdAt: 1_000, attempts: 1 });
    // When the drain runs at t=2500 (retried's dueAt is 3000, fresh is already due)
    const next = nextDue([fresh, retried], 2_500);
    // Then the fresh item is selected because the retried one is still inside its backoff
    expect(next).toBe(fresh);
  });

  it('given a custom backoff shorter than the default, when the drain runs, then the item is due under the custom rule', () => {
    // Given a single item with one attempt and a custom 500ms-per-attempt backoff
    const item = makeItem({ id: 'item-1', createdAt: 1_000, attempts: 1 });
    // When the drain runs at t=2000 with the custom backoff
    const next = nextDue([item], 2_000, (attempts) => attempts * 500);
    // Then the custom backoff (1*500=500) has elapsed and the item is due
    expect(next).toBe(item);
  });
});

describe('OutboxItemSchema', () => {
  it('given a valid outbox item, when parsed through the schema, then the value round-trips', () => {
    // Given a valid outbox item
    const item = makeItem();
    // When parsed
    const parsed = OutboxItemSchema.parse(item);
    // Then the parsed value is structurally identical
    expect(parsed).toEqual(item);
  });
});

describe('toWsUrl', () => {
  it('given an http base URL, when converted, then the scheme becomes ws and the path is /ws', () => {
    // Given an http agent base URL
    const base = 'http://localhost:8787';
    // When converted
    const ws = toWsUrl(base);
    // Then the URL is ws://localhost:8787/ws
    expect(ws).toBe('ws://localhost:8787/ws');
  });

  it('given an https base URL, when converted, then the scheme becomes wss', () => {
    // Given an https agent base URL
    const base = 'https://agent.example.com';
    // When converted
    const ws = toWsUrl(base);
    // Then the URL is wss://agent.example.com/ws
    expect(ws).toBe('wss://agent.example.com/ws');
  });

  it('given a base URL with a trailing slash, when converted, then no double slash appears before /ws', () => {
    // Given an http agent base URL with a trailing slash
    const base = 'http://localhost:8787/';
    // When converted
    const ws = toWsUrl(base);
    // Then there is no double slash before the /ws path
    expect(ws).toBe('ws://localhost:8787/ws');
  });

  it('given a base URL and a token, when converted, then the token is appended as ?token=', () => {
    // Given an https base URL and a bearer token
    const base = 'https://agent.example.com';
    // When converted with the token
    const ws = toWsUrl(base, 'secret-abc');
    // Then the URL ends with ?token=secret-abc
    expect(ws).toBe('wss://agent.example.com/ws?token=secret-abc');
  });

  it('given a non-http(s) base URL, when converted, then an error is thrown', () => {
    // Given an unsupported protocol
    const base = 'ftp://agent.example.com';
    // When converted
    // Then an error is thrown
    expect(() => toWsUrl(base)).toThrow();
  });
});
