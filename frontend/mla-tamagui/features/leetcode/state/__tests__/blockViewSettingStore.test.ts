import AsyncStorage from '@react-native-async-storage/async-storage';
import { describe, expect, it, vi } from 'vitest';

import { DEFAULT_BLOCK_VIEW, useBlockViewSettingStore } from '../blockViewSettingStore';

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

describe('useBlockViewSettingStore', () => {
  it('Given no stored value, When the store is read, Then the block view is off', async () => {
    await useBlockViewSettingStore.persist.rehydrate();

    expect(useBlockViewSettingStore.getState().blockView).toBe(false);
    expect(DEFAULT_BLOCK_VIEW).toBe(false);
  });

  it('Given a stored true, When the store rehydrates, Then the block view is on', async () => {
    await AsyncStorage.setItem('leetcode-block-view', JSON.stringify({ state: { blockView: true }, version: 1 }));

    await useBlockViewSettingStore.persist.rehydrate();

    expect(useBlockViewSettingStore.getState().blockView).toBe(true);
  });

  it('Given a stored value that is not a boolean, When the store rehydrates, Then it falls back to off', async () => {
    await AsyncStorage.setItem('leetcode-block-view', JSON.stringify({ state: { blockView: 'yes' }, version: 1 }));

    await useBlockViewSettingStore.persist.rehydrate();

    expect(useBlockViewSettingStore.getState().blockView).toBe(DEFAULT_BLOCK_VIEW);
  });
});
