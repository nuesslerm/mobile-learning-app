import { __resetLoggerConfig, configureLogger } from '@lib/logging';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { createGlassHeaderStore, DEFAULT_GLASS_HEADERS, GLASS_HEADERS_STORAGE_KEY } from '../lib/createGlassHeaderStore';

type Line = { level: string; message: string; error?: unknown };

function captureLog(): Line[] {
  const lines: Line[] = [];
  const chain = (error?: unknown) => {
    const next: Record<string, unknown> = {
      withContext: () => next,
      withMetadata: () => next,
      withError: (err: unknown) => chain(err),
    };
    for (const level of ['info', 'debug', 'warn', 'error']) {
      next[level] = (message: string) => void lines.push({ level, message, error });
    }
    return next;
  };
  // minimal LogLayer stand-in: the test reads what reached the sink
  configureLogger({ root: () => chain() as never, scrub: false });
  return lines;
}

function memoryStorage() {
  const items = new Map<string, string>();
  const storage = {
    readFails: false,
    items,
    getItem: async (key: string) => {
      if (storage.readFails) throw new Error('storage unavailable');
      return items.get(key) ?? null;
    },
    setItem: async (key: string, value: string) => void items.set(key, value),
    removeItem: async (key: string) => void items.delete(key),
  };
  return storage;
}

describe('the glass-headers store', () => {
  let storage: ReturnType<typeof memoryStorage>;
  let useStore: ReturnType<typeof createGlassHeaderStore>;

  beforeEach(() => {
    storage = memoryStorage();
    useStore = createGlassHeaderStore(storage);
  });

  afterEach(() => {
    __resetLoggerConfig();
  });

  it('Given no stored value, When the store rehydrates, Then glass headers are off', async () => {
    await useStore.persist.rehydrate();

    expect(useStore.getState().enabled).toBe(false);
    expect(DEFAULT_GLASS_HEADERS).toBe(false);
  });

  it('Given a stored true, When the store rehydrates, Then glass headers are on', async () => {
    storage.items.set(GLASS_HEADERS_STORAGE_KEY, JSON.stringify({ state: { enabled: true }, version: 1 }));

    await useStore.persist.rehydrate();

    expect(useStore.getState().enabled).toBe(true);
  });

  it('Given the person turns the setting on, When the app starts again over the same storage, Then it is still on', async () => {
    useStore.getState().setEnabled(true);

    const restarted = createGlassHeaderStore(storage);
    await restarted.persist.rehydrate();

    expect(restarted.getState().enabled).toBe(true);
  });

  it('Given a stored value that is not a boolean, When the store rehydrates, Then glass headers are off', async () => {
    storage.items.set(GLASS_HEADERS_STORAGE_KEY, JSON.stringify({ state: { enabled: 'yes' }, version: 1 }));

    await useStore.persist.rehydrate();

    expect(useStore.getState().enabled).toBe(false);
  });

  it('Given the storage cannot be read, When the store rehydrates, Then glass headers are off and the failure is logged with its error', async () => {
    const lines = captureLog();
    storage.readFails = true;

    await useStore.persist.rehydrate();

    expect(useStore.getState().enabled).toBe(false);
    expect(lines.find((line) => line.message === 'glassHeaders.rehydrate_failed')).toEqual({
      level: 'error',
      message: 'glassHeaders.rehydrate_failed',
      error: new Error('storage unavailable'),
    });
  });
});
