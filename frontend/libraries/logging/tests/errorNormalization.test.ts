import { describe, expect, it } from 'vitest';

import { Logger, configureLogger } from '../src/logger/index.js';

/**
 * Guards the one thing that made every catch-block log useless: `Error`'s
 * `message`/`stack`/`name` are NON-ENUMERABLE, so an Error placed in metadata
 * serializes as `{}` through any JSON transport — the event arrives, the reason
 * it was logged does not. This repo's convention for catch blocks is
 * `withMetadata({ err })`, so the normalisation has to live in the logger
 * rather than at each call site.
 */
function captureMetadata(emit: () => void): Record<string, unknown> {
  const captured: Record<string, unknown>[] = [];
  const chain: Record<string, unknown> = {
    withContext: () => chain,
    withError: () => chain,
    withMetadata: (meta: Record<string, unknown>) => {
      captured.push(meta);
      return chain;
    },
    info: () => undefined,
    debug: () => undefined,
    warn: () => undefined,
    error: () => undefined,
  };
  // eslint-disable-next-line @typescript-eslint/no-explicit-any -- minimal LogChain stand-in
  configureLogger({ root: () => chain as any });
  emit();
  return captured[0] ?? {};
}

describe('given an Error placed in log metadata', () => {
  it('when the event is emitted, then the message survives JSON serialization', () => {
    const meta = captureMetadata(() =>
      Logger.withMetadata({ err: new Error('boom'), other: 1 }).error('probe.event')
    );

    // The actual failure mode: stringify used to yield {"err":{}}.
    expect(JSON.stringify(meta)).toContain('boom');
    expect(meta['other']).toBe(1);

    const err = meta['err'] as { name?: string; message?: string; stack?: string };
    expect(err.message).toBe('boom');
    expect(err.name).toBe('Error');
    expect(err.stack).toBeTypeOf('string');
  });

  it('when the Error is a subclass, then its own name is kept', () => {
    class TimeoutError extends Error {
      constructor() {
        super('timed out');
        this.name = 'TimeoutError';
      }
    }

    const meta = captureMetadata(() =>
      Logger.withMetadata({ err: new TimeoutError() }).error('probe.subclass')
    );

    const err = meta['err'] as { name?: string; message?: string };
    expect(err.name).toBe('TimeoutError');
    expect(err.message).toBe('timed out');
  });

  it('when it is logged through the static API (no babel rewrite), then the message survives too', () => {
    const meta = captureMetadata(() => Logger.error('probe.static', { error: new Error('boom'), kind: 'error' }));

    expect(meta['kind']).toBe('error');
    expect(meta['error']).toMatchObject({ name: 'Error', message: 'boom' });
    expect(JSON.stringify(meta)).toContain('boom');
  });

  it('when metadata holds no Error, then it is passed through untouched', () => {
    const fields = { a: 1, b: 'two' };

    const meta = captureMetadata(() => Logger.withMetadata(fields).info('probe.plain'));

    expect(meta).toEqual(fields);
  });
});
