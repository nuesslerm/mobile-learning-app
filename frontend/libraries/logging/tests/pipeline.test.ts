import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { __resetLiveTail, liveTail, type LogEntry } from '../src/liveTail';
import {
  ConsoleEchoTransport,
  PipelineTransport,
  appendToLiveTail,
  toStoreLevel,
  type Stage,
} from '../src/pipeline';

beforeEach(() => {
  __resetLiveTail();
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

/**
 * Helper: build a minimal LogLayerTransportParams object for testing.
 */
function params(
  logLevel: 'info' | 'warn' | 'error' | 'debug' | 'fatal',
  messages: unknown[],
  data?: Record<string, unknown> | null,
  hasData = data !== undefined && data !== null
): import('@loglayer/transport').LogLayerTransportParams {
  return {
    logLevel,
    messages,
    data: data ?? undefined,
    hasData,
  };
}

function serializeToLogEntry(
  p: import('@loglayer/transport').LogLayerTransportParams
): LogEntry {
  return {
    id: 0,
    ts: Date.now(),
    level: toStoreLevel(p.logLevel),
    text: String(p.messages[0] ?? ''),
    context: {},
  };
}

describe('toStoreLevel', () => {
  it('passes standard levels through', () => {
    expect(toStoreLevel('debug')).toBe('debug');
    expect(toStoreLevel('info')).toBe('info');
    expect(toStoreLevel('warn')).toBe('warn');
    expect(toStoreLevel('error')).toBe('error');
  });

  it('maps trace → debug and fatal → error', () => {
    expect(toStoreLevel('trace')).toBe('debug');
    expect(toStoreLevel('fatal')).toBe('error');
  });
});

describe('PipelineTransport — stages compose from the same batch', () => {
  it('runs every stage with the same batch reference (read-only contract)', () => {
    const calls: LogEntry[][] = [];
    const stageA: Stage<LogEntry> = (batch) => {
      calls.push([...batch]);
    };
    const stageB: Stage<LogEntry> = (batch) => {
      calls.push([...batch]);
    };

    const transport = new PipelineTransport<LogEntry>({
      serialize: serializeToLogEntry,
      stages: [stageA, stageB],
    });

    transport.shipToLogger(params('info', ['hello']));
    vi.advanceTimersByTime(1000);

    expect(calls).toHaveLength(2);
    expect(calls[0]).toEqual(calls[1]);
  });

  it('runs every stage per buffered flush', () => {
    const seen: string[] = [];
    const transport = new PipelineTransport<LogEntry>({
      serialize: serializeToLogEntry,
      stages: [
        (batch) => {
          for (const e of batch) seen.push(`a:${e.text}`);
        },
        (batch) => {
          for (const e of batch) seen.push(`b:${e.text}`);
        },
      ],
    });

    transport.shipToLogger(params('info', ['x']));
    transport.shipToLogger(params('info', ['y']));
    vi.advanceTimersByTime(1000);

    expect(seen).toEqual(['a:x', 'a:y', 'b:x', 'b:y']);
  });

  it('does not crash if a stage throws (defensive)', () => {
    const transport = new PipelineTransport<LogEntry>({
      serialize: serializeToLogEntry,
      stages: [
        () => {
          throw new Error('stage boom');
        },
      ],
    });

    expect(() => {
      transport.shipToLogger(params('info', ['x']));
      vi.advanceTimersByTime(1000);
    }).not.toThrow();
  });
});

describe('appendToLiveTail stage', () => {
  it('appends the batch to liveTail', async () => {
    const stage = appendToLiveTail();
    await stage([
      { level: 'info', text: 'a', source: 'Module', context: { foo: 1 } },
    ]);
    const entries = liveTail.getEntries();
    expect(entries).toHaveLength(1);
    expect(entries[0]!.text).toBe('a');
    expect(entries[0]!.source).toBe('Module');
    expect(entries[0]!.context).toEqual({ foo: 1 });
  });

  it('is a no-op on an empty batch', async () => {
    const before = liveTail.getVersion();
    await appendToLiveTail()([]);
    expect(liveTail.getVersion()).toBe(before);
  });
});

describe('ConsoleEchoTransport — echoes to the injected console', () => {
  it('echoes via the configured console sink on flush', () => {
    const log = vi.fn();
    const transport = new ConsoleEchoTransport({ console: { log } });
    transport.shipToLogger(params('info', ['hello']));
    vi.advanceTimersByTime(1000);
    expect(log).toHaveBeenCalledWith('hello');
    const call = log.mock.calls[0];
    expect(call).toEqual(['hello']);
  });

  it('Given a filtered console sink, When a batch flushes, Then it receives only accepted events', () => {
    const info = vi.fn();
    const transport = new ConsoleEchoTransport({
      id: 'diagnostics',
      console: { info },
      accepts: (event) => event === 'editor.input_timing',
    });

    transport.shipToLogger(params('info', ['app.boot']));
    transport.shipToLogger(params('info', ['editor.input_timing']));
    vi.advanceTimersByTime(1000);

    expect(info.mock.calls).toEqual([['editor.input_timing']]);
  });

  it('falls back to console.log when the level-specific method is missing', () => {
    const log = vi.fn();
    const transport = new ConsoleEchoTransport({ console: { log } });
    transport.shipToLogger(params('debug', ['hi']));
    vi.advanceTimersByTime(1000);
    expect(log).toHaveBeenCalledWith('hi');
    const call = log.mock.calls[0];
    expect(call).toEqual(['hi']);
  });

  it('appends the data object to the message args when hasData is true', () => {
    const log = vi.fn();
    const transport = new ConsoleEchoTransport({ console: { log } });
    transport.shipToLogger(params('info', ['event'], { userId: 1 }, true));
    vi.advanceTimersByTime(1000);
    expect(log).toHaveBeenCalledWith('event', { userId: 1 });
    const call = log.mock.calls[0];
    expect(call).toEqual(['event', { userId: 1 }]);
  });
});
