import AsyncStorage from '@react-native-async-storage/async-storage';
import { describe, expect, it, vi } from 'vitest';

import { DEFAULT_CONSOLE, useConsoleSettingStore } from '../consoleSettingStore';

// the real package's web shim reaches for `window`, which this node suite does not have: an in-memory storage stands in for it
vi.mock('@react-native-async-storage/async-storage', () => {
  const store = new Map<string, string>();
  return {
    default: {
      getItem: async (key: string) => store.get(key) ?? null,
      setItem: async (key: string, value: string) => void store.set(key, value),
      removeItem: async (key: string) => void store.delete(key),
    },
  };
});

describe('useConsoleSettingStore', () => {
  it('Given no stored value, When the store is rehydrated, Then the console is off', async () => {
    await useConsoleSettingStore.persist.rehydrate();

    expect(useConsoleSettingStore.getState().enabled).toBe(false);
    expect(DEFAULT_CONSOLE).toBe(false);
  });

  it('Given a stored true, When the store is rehydrated, Then the console is on', async () => {
    await AsyncStorage.setItem('leetcode-console', JSON.stringify({ state: { enabled: true }, version: 1 }));

    await useConsoleSettingStore.persist.rehydrate();

    expect(useConsoleSettingStore.getState().enabled).toBe(true);
  });

  it('Given a stored value that is not a boolean, When the store rehydrates, Then it falls back to off', async () => {
    await AsyncStorage.setItem('leetcode-console', JSON.stringify({ state: { enabled: 'yes' }, version: 1 }));

    await useConsoleSettingStore.persist.rehydrate();

    expect(useConsoleSettingStore.getState().enabled).toBe(DEFAULT_CONSOLE);
  });
});
