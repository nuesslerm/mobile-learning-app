import AsyncStorage from '@react-native-async-storage/async-storage';
import { describe, expect, it, vi } from 'vitest';

import { DEFAULT_RUNNER, useRunnerSettingStore, whenRunnerSettingHydrated } from '../runnerSettingStore';

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

describe('useRunnerSettingStore', () => {
  it('Given no stored value, When the store is read, Then Run Local uses this device', async () => {
    await whenRunnerSettingHydrated();
    expect(useRunnerSettingStore.getState().runner).toBe('device');
    expect(DEFAULT_RUNNER).toBe('device');
  });

  it('Given the person picks the server, When the store is rehydrated from storage, Then the choice survives', async () => {
    await AsyncStorage.setItem('leetcode-runner', JSON.stringify({ state: { runner: 'backend' }, version: 1 }));
    await useRunnerSettingStore.persist.rehydrate();
    expect(useRunnerSettingStore.getState().runner).toBe('backend');
  });

  it('Given a stored value that is neither runner, When the store rehydrates, Then it falls back to the default', async () => {
    await AsyncStorage.setItem('leetcode-runner', JSON.stringify({ state: { runner: 'quantum' }, version: 1 }));
    await useRunnerSettingStore.persist.rehydrate();
    expect(useRunnerSettingStore.getState().runner).toBe(DEFAULT_RUNNER);
  });
});
