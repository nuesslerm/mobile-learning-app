// Unit tests for the in-memory liveTail's snapshot contract.
//
// Why these tests matter (see `liveTail.ts` for the full contract doc):
//   • `getEntries()` MUST return a NEW readonly array on every mutation, or
//     `useSyncExternalStore`'s `Object.is` snapshot check bails and the
//     overlay stops re-rendering.
//   • Listeners must receive `(snapshot, prev)` and must be coalesced
//     (one emit per microtask, regardless of how many `add` calls landed
//     in the burst).
//   • `getVersion()` must increment on every mutation.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { __resetLiveTail, liveTail, safeJson } from '../src/liveTail';

beforeEach(async () => {
  vi.useFakeTimers();
  __resetLiveTail();
  await vi.runOnlyPendingTimersAsync();
});

afterEach(async () => {
  __resetLiveTail();
  vi.useRealTimers();
});

describe('liveTail.add — snapshot contract', () => {
  it('returns a new array reference on every add (immutable snapshot)', () => {
    const before = liveTail.getEntries();
    liveTail.add('info', 'hello');
    const after = liveTail.getEntries();
    expect(after).not.toBe(before);
    expect(before).toEqual([]);
    expect(after).toHaveLength(1);
  });

  it('appends to the snapshot, not mutates in place', () => {
    liveTail.add('info', 'a');
    const snap1 = liveTail.getEntries();
    liveTail.add('info', 'b');
    const snap2 = liveTail.getEntries();
    expect(snap1).toHaveLength(1);
    expect(snap2).toHaveLength(2);
    expect(snap1[0]!.text).toBe('a');
    expect(snap2[0]!.text).toBe('a');
    expect(snap2[1]!.text).toBe('b');
  });

  it('preserves source + context on the entry', () => {
    const entry = liveTail.add('warn', 'careful', {
      source: 'EditorInput',
      context: { foo: 1 },
    });
    expect(entry.source).toBe('EditorInput');
    expect(entry.context).toEqual({ foo: 1 });
  });

  it('omits context when opts.context is an empty object', () => {
    const entry = liveTail.add('info', 'no ctx', { context: {} });
    expect(entry.context).toBeUndefined();
  });
});

describe('liveTail.getVersion', () => {
  it('increments on every add', () => {
    const v0 = liveTail.getVersion();
    liveTail.add('info', 'a');
    const v1 = liveTail.getVersion();
    liveTail.add('info', 'b');
    const v2 = liveTail.getVersion();
    expect(v1).toBeGreaterThan(v0);
    expect(v2).toBeGreaterThan(v1);
  });

  it('increments on clear', () => {
    liveTail.add('info', 'a');
    const before = liveTail.getVersion();
    liveTail.clear();
    const after = liveTail.getVersion();
    expect(after).toBeGreaterThan(before);
  });
});

describe('liveTail.clear', () => {
  it('empties the snapshot and produces a new ref', () => {
    liveTail.add('info', 'a');
    liveTail.add('info', 'b');
    const before = liveTail.getEntries();
    liveTail.clear();
    const after = liveTail.getEntries();
    expect(after).toEqual([]);
    expect(after).not.toBe(before);
  });

  it('resets nextId so a subsequent add returns id 1', () => {
    liveTail.add('info', 'a');
    liveTail.add('info', 'b');
    liveTail.clear();
    const fresh = liveTail.add('info', 'after clear');
    expect(fresh.id).toBe(1);
  });
});

describe('liveTail.appendBatch', () => {
  it('adds N entries in a single mutation (one version bump)', () => {
    const v0 = liveTail.getVersion();
    const built = liveTail.appendBatch([
      { level: 'info', text: 'a' },
      { level: 'info', text: 'b' },
      { level: 'info', text: 'c' },
    ]);
    const v1 = liveTail.getVersion();
    expect(built).toHaveLength(3);
    expect(v1 - v0).toBe(1);
    expect(liveTail.getEntries()).toHaveLength(3);
  });

  it('is a no-op (no emit, no version bump) on an empty batch', () => {
    const v0 = liveTail.getVersion();
    const built = liveTail.appendBatch([]);
    expect(built).toEqual([]);
    expect(liveTail.getVersion()).toBe(v0);
  });
});

describe('liveTail.subscribe', () => {
  it('returns an unsubscribe function that detaches the listener', async () => {
    const listener = vi.fn();
    const off = liveTail.subscribe(listener);
    liveTail.add('info', 'a');
    await vi.advanceTimersByTimeAsync(1001);
    expect(listener).toHaveBeenCalledTimes(1);
    const [snapshotArg] = listener.mock.calls[0] ?? [];
    expect(snapshotArg).toHaveLength(1);
    expect(snapshotArg?.[0]?.text).toBe('a');
    off();
    liveTail.add('info', 'b');
    await vi.advanceTimersByTimeAsync(1001);
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it('continues calling other listeners if one throws', async () => {
    const safe = vi.fn();
    liveTail.subscribe(() => {
      throw new Error('listener boom');
    });
    liveTail.subscribe(safe);
    liveTail.add('info', 'a');
    await vi.advanceTimersByTimeAsync(1001);
    expect(safe).toHaveBeenCalledTimes(1);
    const [snapshotArg] = safe.mock.calls[0] ?? [];
    expect(snapshotArg).toHaveLength(1);
    expect(snapshotArg?.[0]?.text).toBe('a');
  });
});

describe('safeJson', () => {
  it('JSON-stringifies a plain object', () => {
    expect(safeJson({ foo: 1, bar: 'baz' })).toBe('{"foo":1,"bar":"baz"}');
  });

  it('falls back to String() for circular references', () => {
    const circular: any = {};
    circular.self = circular;
    expect(safeJson(circular)).toBe(String(circular));
  });
});
