import AsyncStorage from '@react-native-async-storage/async-storage';
import { describe, expect, it, vi } from 'vitest';

import { DEFAULT_CURSOR_HANDLE, useCursorHandleSettingStore } from '../editor/cursorHandleSettingStore';

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

describe('useCursorHandleSettingStore', () => {
  it('Given no stored value, When the store is read, Then the handle is off', async () => {
    await useCursorHandleSettingStore.persist.rehydrate();

    expect(useCursorHandleSettingStore.getState().cursorHandle).toBe(false);
    expect(DEFAULT_CURSOR_HANDLE).toBe(false);
  });

  it('Given the person turns the handle on, When the store is rehydrated from storage, Then the choice survives', async () => {
    await AsyncStorage.setItem('leetcode-cursor-handle', JSON.stringify({ state: { cursorHandle: true }, version: 1 }));

    await useCursorHandleSettingStore.persist.rehydrate();

    expect(useCursorHandleSettingStore.getState().cursorHandle).toBe(true);
  });

  it('Given a stored value that is not a boolean, When the store rehydrates, Then it falls back to the default', async () => {
    await AsyncStorage.setItem('leetcode-cursor-handle', JSON.stringify({ state: { cursorHandle: 'yes' }, version: 1 }));

    await useCursorHandleSettingStore.persist.rehydrate();

    expect(useCursorHandleSettingStore.getState().cursorHandle).toBe(DEFAULT_CURSOR_HANDLE);
  });
});
