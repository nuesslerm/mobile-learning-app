import AsyncStorage from '@react-native-async-storage/async-storage';
import { describe, expect, it, vi } from 'vitest';

import { DEFAULT_TOOLBAR_ITEMS } from '../../domain';
import { useToolbarConfigStore } from '../toolbarConfigStore';

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

const KEY = 'leetcode-toolbar-config';
const custom = { kind: 'custom', id: 'c1', label: '->', insert: '->' };

describe('useToolbarConfigStore', () => {
  it('Given the default toolbar, When it is listed, Then no key is Rename', () => {
    expect(DEFAULT_TOOLBAR_ITEMS.some((item) => item.kind === 'builtin' && (item.id as string) === 'RenameSymbol')).toBe(false);
  });

  it('Given a stored v1 toolbar with the Rename key and a custom key, When it rehydrates, Then the custom key stays and Rename is gone', async () => {
    const items = [{ kind: 'builtin', id: 'Tab' }, { kind: 'builtin', id: 'RenameSymbol' }, custom, { kind: 'builtin', id: 'Dot' }];
    await AsyncStorage.setItem(KEY, JSON.stringify({ state: { items }, version: 1 }));

    await useToolbarConfigStore.persist.rehydrate();

    expect(useToolbarConfigStore.getState().items).toEqual([{ kind: 'builtin', id: 'Tab' }, custom, { kind: 'builtin', id: 'Dot' }]);
  });

  it('Given a stored v2 toolbar, When it rehydrates, Then it is kept as it is', async () => {
    const items = [custom, { kind: 'builtin', id: 'Undo' }];
    await AsyncStorage.setItem(KEY, JSON.stringify({ state: { items }, version: 2 }));

    await useToolbarConfigStore.persist.rehydrate();

    expect(useToolbarConfigStore.getState().items).toEqual(items);
  });

  it('Given a stored toolbar that names a key that does not exist, When it rehydrates, Then the toolbar the app was showing stays', async () => {
    useToolbarConfigStore.getState().resetToDefault();
    await AsyncStorage.setItem(KEY, JSON.stringify({ state: { items: [{ kind: 'builtin', id: 'Teleport' }] }, version: 2 }));

    await useToolbarConfigStore.persist.rehydrate();

    expect(useToolbarConfigStore.getState().items).toEqual(DEFAULT_TOOLBAR_ITEMS);
  });
});
