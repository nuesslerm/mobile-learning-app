import { __resetLoggerConfig, configureLogger } from '@lib/logging';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { createNativeHeaderStore, DEFAULT_NATIVE_HEADERS, NATIVE_HEADERS_STORAGE_KEY } from '../lib/createNativeHeaderStore';

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

describe('the native-headers store', () => {
  let storage: ReturnType<typeof memoryStorage>;
  let useStore: ReturnType<typeof createNativeHeaderStore>;

  beforeEach(() => {
    storage = memoryStorage();
    useStore = createNativeHeaderStore(storage);
  });

  afterEach(() => {
    __resetLoggerConfig();
  });

  it('Given no stored value, When the store rehydrates, Then native headers are on', async () => {
    await useStore.persist.rehydrate();

    expect(useStore.getState().enabled).toBe(true);
    expect(DEFAULT_NATIVE_HEADERS).toBe(true);
  });

  it('Given a stored false, When the store rehydrates, Then native headers are off', async () => {
    storage.items.set(NATIVE_HEADERS_STORAGE_KEY, JSON.stringify({ state: { enabled: false }, version: 1 }));

    await useStore.persist.rehydrate();

    expect(useStore.getState().enabled).toBe(false);
  });

  it('Given the old glass-headers key holding false, When the store rehydrates, Then native headers are on', async () => {
    storage.items.set('glass-headers', JSON.stringify({ state: { enabled: false }, version: 1 }));

    await useStore.persist.rehydrate();

    expect(useStore.getState().enabled).toBe(true);
  });

  it('Given the person turns the setting off, When the app starts again over the same storage, Then it is still off', async () => {
    useStore.getState().setEnabled(false);

    const restarted = createNativeHeaderStore(storage);
    await restarted.persist.rehydrate();

    expect(restarted.getState().enabled).toBe(false);
  });

  it('Given a stored value that is not a boolean, When the store rehydrates, Then native headers are on', async () => {
    storage.items.set(NATIVE_HEADERS_STORAGE_KEY, JSON.stringify({ state: { enabled: 'no' }, version: 1 }));

    await useStore.persist.rehydrate();

    expect(useStore.getState().enabled).toBe(true);
  });

  it('Given the storage cannot be read, When the store rehydrates, Then native headers are on and the failure is logged with its error', async () => {
    const lines = captureLog();
    storage.readFails = true;

    await useStore.persist.rehydrate();

    expect(useStore.getState().enabled).toBe(true);
    expect(lines.find((line) => line.message === 'nativeHeaders.rehydrate_failed')).toEqual({
      level: 'error',
      message: 'nativeHeaders.rehydrate_failed',
      error: new Error('storage unavailable'),
    });
  });
});
