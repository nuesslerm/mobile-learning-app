import AsyncStorage from '@react-native-async-storage/async-storage';
import { describe, expect, it, vi } from 'vitest';

import { DEFAULT_AUTOCOMPLETE, useAutocompleteSettingStore } from '../autocompleteSettingStore';

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

describe('useAutocompleteSettingStore', () => {
  it('Given no stored value, When the store is read, Then suggestions are off', async () => {
    await useAutocompleteSettingStore.persist.rehydrate();

    expect(useAutocompleteSettingStore.getState().autocomplete).toBe(false);
    expect(DEFAULT_AUTOCOMPLETE).toBe(false);
  });

  it('Given the person turns suggestions on, When the store is rehydrated from storage, Then the choice survives', async () => {
    await AsyncStorage.setItem('leetcode-autocomplete', JSON.stringify({ state: { autocomplete: true }, version: 1 }));

    await useAutocompleteSettingStore.persist.rehydrate();

    expect(useAutocompleteSettingStore.getState().autocomplete).toBe(true);
  });

  it('Given a stored value that is not a boolean, When the store rehydrates, Then it falls back to off', async () => {
    await AsyncStorage.setItem('leetcode-autocomplete', JSON.stringify({ state: { autocomplete: 'yes' }, version: 1 }));

    await useAutocompleteSettingStore.persist.rehydrate();

    expect(useAutocompleteSettingStore.getState().autocomplete).toBe(DEFAULT_AUTOCOMPLETE);
  });
});
