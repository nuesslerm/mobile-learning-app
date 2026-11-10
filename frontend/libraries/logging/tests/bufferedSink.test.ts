import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createBufferedSink, drainAllBufferedSinks, flushAllBufferedSinks } from '../src/primitives';

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('createBufferedSink — basic flush behavior', () => {
  it('flushes a batch on the configured cadence', () => {
    const onFlush = vi.fn();
    const sink = createBufferedSink<number>({
      flushMs: 1000,
      maxBuffer: 100,
      onFlush,
    });

    sink.push(1);
    sink.push(2);
    sink.push(3);
    expect(onFlush).not.toHaveBeenCalled();

    vi.advanceTimersByTime(1000);
    expect(onFlush).toHaveBeenCalledTimes(1);
    expect(onFlush).toHaveBeenCalledWith([1, 2, 3]);
    const flushedBatch = onFlush.mock.calls[0]?.[0];
    expect(flushedBatch).toEqual([1, 2, 3]);
  });

  it('drains immediately when the cap is hit', () => {
    const onFlush = vi.fn();
    const sink = createBufferedSink<number>({
      flushMs: 1000,
      maxBuffer: 3,
      onFlush,
    });

    sink.push(1);
    sink.push(2);
    sink.push(3); // hits cap → drain
    expect(onFlush).toHaveBeenCalledTimes(1);
    expect(onFlush).toHaveBeenLastCalledWith([1, 2, 3]);
    const drainedBatch = onFlush.mock.calls[0]?.[0];
    expect(drainedBatch).toEqual([1, 2, 3]);
  });

  it('flushNow() drains pending items immediately and clears the timer', () => {
    const onFlush = vi.fn();
    const sink = createBufferedSink<number>({
      flushMs: 1000,
      maxBuffer: 100,
      onFlush,
    });

    sink.push(1);
    sink.flushNow();
    expect(onFlush).toHaveBeenCalledTimes(1);
    expect(onFlush).toHaveBeenCalledWith([1]);
    const drainedBatch = onFlush.mock.calls[0]?.[0];
    expect(drainedBatch).toEqual([1]);

    // No additional flush on the timer — the timer was cleared.
    vi.advanceTimersByTime(1000);
    expect(onFlush).toHaveBeenCalledTimes(1);
  });

  it('flushNow() is a no-op when buffer is empty', () => {
    const onFlush = vi.fn();
    const sink = createBufferedSink<number>({
      flushMs: 1000,
      maxBuffer: 100,
      onFlush,
    });

    sink.flushNow();
    expect(onFlush).not.toHaveBeenCalled();

    // The no-op flush must not have corrupted internal state -- the sink
    // still buffers and drains a real item correctly afterward.
    sink.push(42);
    sink.flushNow();
    const flushedBatch = onFlush.mock.calls[0]?.[0];
    expect(flushedBatch).toEqual([42]);
  });

  it('does not throw if onFlush throws (defensive)', () => {
    const sink = createBufferedSink<number>({
      flushMs: 1000,
      maxBuffer: 100,
      onFlush: () => {
        throw new Error('handler boom');
      },
    });

    sink.push(1);
    expect(() => {
      sink.flushNow();
    }).not.toThrow();
  });
});

describe('createBufferedSink — crash-path drain via flushAllBufferedSinks', () => {
  it('drains every sink that opted into register: true', () => {
    const onFlushA = vi.fn();
    const onFlushB = vi.fn();
    const sinkA = createBufferedSink<number>({
      flushMs: 1000,
      maxBuffer: 100,
      register: true,
      onFlush: onFlushA,
    });
    const sinkB = createBufferedSink<number>({
      flushMs: 1000,
      maxBuffer: 100,
      register: true,
      onFlush: onFlushB,
    });
    createBufferedSink<string>({
      flushMs: 1000,
      maxBuffer: 100,
      // register: false → NOT drained on crash
      onFlush: vi.fn(),
    });

    // Push items so each registered sink has something to flush.
    sinkA.push(1);
    sinkA.push(2);
    sinkB.push(99);

    flushAllBufferedSinks();
    expect(onFlushA).toHaveBeenCalledTimes(1);
    expect(onFlushA).toHaveBeenCalledWith([1, 2]);
    expect(onFlushB).toHaveBeenCalledTimes(1);
    expect(onFlushB).toHaveBeenCalledWith([99]);
    const batchA = onFlushA.mock.calls[0]?.[0];
    const batchB = onFlushB.mock.calls[0]?.[0];
    expect(batchA).toEqual([1, 2]);
    expect(batchB).toEqual([99]);
  });

  it('continues draining even if one onFlush throws', () => {
    const reached = vi.fn();
    const boomSink = createBufferedSink<number>({
      flushMs: 1000,
      maxBuffer: 100,
      register: true,
      onFlush: () => {
        throw new Error('boom');
      },
    });
    const safeSink = createBufferedSink<number>({
      flushMs: 1000,
      maxBuffer: 100,
      register: true,
      onFlush: reached,
    });

    boomSink.push(1);
    safeSink.push(2);

    expect(() => flushAllBufferedSinks()).not.toThrow();
    expect(reached).toHaveBeenCalledTimes(1);
    expect(reached).toHaveBeenCalledWith([2]);
  });
});

describe('drainAllBufferedSinks — waiting for the crash drain to land', () => {
  function heldSend() {
    let release: () => void = () => undefined;
    const sent = new Promise<void>((resolve) => {
      release = resolve;
    });
    return { sent, release: () => release() };
  }

  it('given a send in flight, when the drain runs, then it settles only once that send has settled', async () => {
    // Given a registered sink whose send is held
    const send = heldSend();
    const sink = createBufferedSink<number>({ flushMs: 1000, maxBuffer: 100, register: true, onFlush: () => send.sent });
    sink.push(1);

    // When every sink is drained
    let settled = false;
    void drainAllBufferedSinks().then(() => {
      settled = true;
    });
    await Promise.resolve();
    const whileHeld = settled;
    send.release();
    await vi.waitFor(() => expect(settled).toBe(true));

    // Then the drain waited for the send
    expect(whileHeld).toBe(false);
  });

  it('given a send that never settles, when the drain runs with a timeout, then it settles at the timeout', async () => {
    // Given a registered sink whose send hangs
    const sink = createBufferedSink<number>({
      flushMs: 1000,
      maxBuffer: 100,
      register: true,
      onFlush: () => new Promise<void>(() => undefined),
    });
    sink.push(1);

    // When every sink is drained with a 2 s bound
    let settled = false;
    void drainAllBufferedSinks(2000).then(() => {
      settled = true;
    });
    await vi.advanceTimersByTimeAsync(1999);
    const before = settled;
    await vi.advanceTimersByTimeAsync(1);

    // Then it gave up waiting at 2 s, not before
    expect(before).toBe(false);
    expect(settled).toBe(true);
  });

  it('given a sink that registers another while it flushes, when the drain runs, then the new sink is drained in the same pass', () => {
    // Given a sink whose flush creates and fills a second registered sink, as a pipeline stage does on its first batch
    const reachedInner = vi.fn();
    const outer = createBufferedSink<number>({
      flushMs: 1000,
      maxBuffer: 100,
      register: true,
      onFlush: (batch) => {
        const inner = createBufferedSink<number>({ flushMs: 1000, maxBuffer: 100, register: true, onFlush: reachedInner });
        for (const n of batch) inner.push(n);
      },
    });
    outer.push(7);

    // When every sink is drained, with no timer advanced
    flushAllBufferedSinks();

    // Then the inner sink's batch went out in that same drain
    expect(reachedInner).toHaveBeenCalledWith([7]);
  });
});
