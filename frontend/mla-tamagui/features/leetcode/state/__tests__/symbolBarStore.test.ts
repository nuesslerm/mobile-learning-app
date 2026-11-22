import AsyncStorage from '@react-native-async-storage/async-storage';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { DEFAULT_SYMBOL_GROUPS, isDefaultGroups, moveGroup } from '../../domain/editor-settings/symbolGroups';
import { DEFAULT_RAIL_STYLE, DEFAULT_SYMBOL_BAR_SHOWN, useSymbolBarStore } from '../editor/symbolBarStore';

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

const KEY = 'leetcode-symbol-bar';
const stored = (state: object) => AsyncStorage.setItem(KEY, JSON.stringify({ state, version: 1 }));
const groupsOf = () => useSymbolBarStore.getState().groups;
const chars = () => groupsOf().map((group) => group.chars);

beforeEach(async () => {
  await AsyncStorage.removeItem(KEY);
  await useSymbolBarStore.persist.rehydrate();
});

describe('useSymbolBarStore', () => {
  it('Given no stored value, When the store is read, Then the bar is shown, the groups are the eight defaults and the rail is the classic one', () => {
    const { shown, groups, railStyle } = useSymbolBarStore.getState();

    expect(shown).toBe(true);
    expect(DEFAULT_SYMBOL_BAR_SHOWN).toBe(true);
    expect(isDefaultGroups(groups)).toBe(true);
    expect(railStyle).toBe('classic');
    expect(DEFAULT_RAIL_STYLE).toBe('classic');
  });

  it('Given a stored choice, When the store rehydrates, Then the switch, the order, the characters and the rail style all hold', async () => {
    const moved = moveGroup(DEFAULT_SYMBOL_GROUPS, 1, 2).map((group) => (group.id === 'g1' ? { ...group, chars: '#' } : group));
    await stored({ shown: false, groups: moved, railStyle: 'symbols' });

    await useSymbolBarStore.persist.rehydrate();

    expect(useSymbolBarStore.getState().shown).toBe(false);
    expect(groupsOf()).toEqual(moved);
    expect(useSymbolBarStore.getState().railStyle).toBe('symbols');
  });

  it('Given a stored switch that is not a boolean, When the store rehydrates, Then that field alone falls back', async () => {
    await stored({ shown: 'yes', groups: moveGroup(DEFAULT_SYMBOL_GROUPS, 0, 7), railStyle: 'symbols' });

    await useSymbolBarStore.persist.rehydrate();

    expect(useSymbolBarStore.getState().shown).toBe(true);
    expect(groupsOf().map((group) => group.id)).toEqual(['g2', 'g3', 'g4', 'g5', 'g6', 'g7', 'g8', 'g1']);
    expect(useSymbolBarStore.getState().railStyle).toBe('symbols');
  });

  it('Given stored groups that are not eight valid ones, When the store rehydrates, Then the groups fall back to the defaults and the other fields hold', async () => {
    await stored({ shown: false, groups: DEFAULT_SYMBOL_GROUPS.slice(0, 7), railStyle: 'symbols' });

    await useSymbolBarStore.persist.rehydrate();

    expect(isDefaultGroups(groupsOf())).toBe(true);
    expect(useSymbolBarStore.getState().shown).toBe(false);
    expect(useSymbolBarStore.getState().railStyle).toBe('symbols');
  });

  it('Given a stored rail style that is neither classic nor symbols, When the store rehydrates, Then it is classic and the other fields hold', async () => {
    await stored({ shown: false, groups: DEFAULT_SYMBOL_GROUPS, railStyle: 'wide' });

    await useSymbolBarStore.persist.rehydrate();

    expect(useSymbolBarStore.getState().railStyle).toBe('classic');
    expect(useSymbolBarStore.getState().shown).toBe(false);
  });

  it('Given a stored value that is not an object, When the store rehydrates, Then every field is its default', async () => {
    await AsyncStorage.setItem(KEY, JSON.stringify({ state: 'broken', version: 1 }));

    await useSymbolBarStore.persist.rehydrate();

    const { shown, groups, railStyle } = useSymbolBarStore.getState();
    expect([shown, isDefaultGroups(groups), railStyle]).toEqual([true, true, 'classic']);
  });

  it('Given a group edited to more than four characters, When it is set, Then it holds the first four and the other groups are untouched', () => {
    useSymbolBarStore.getState().setGroup('g2', 'abcdef');

    expect(chars()).toEqual(['#$^%', 'abcd', '\\@~_', '+-*/', '\'"`=', '{}[]', '()<>', '.:,;']);
  });

  it('Given a group edited to an emoji and spaces, When it is set, Then the spaces are dropped and the emoji counts as one', () => {
    useSymbolBarStore.getState().setGroup('g1', ' \u{1F600} a ');

    expect(groupsOf()[0]).toEqual({ id: 'g1', chars: '\u{1F600}a' });
  });

  it('Given a group edited to nothing, When it is set, Then it is empty, which hides it from the rail', () => {
    useSymbolBarStore.getState().setGroup('g3', '');

    expect(groupsOf()[2]).toEqual({ id: 'g3', chars: '' });
  });

  it('Given a move of group 2 below group 3, When applied, Then the order is the recording\'s and the groups are no longer the defaults', () => {
    useSymbolBarStore.getState().move(1, 2);

    expect(chars().slice(0, 3)).toEqual(['#$^%', '\\@~_', '&|?!']);
    expect(isDefaultGroups(groupsOf())).toBe(false);
  });

  it('Given a move out of range, When applied, Then the groups are the very same list', () => {
    const before = groupsOf();

    useSymbolBarStore.getState().move(0, 9);

    expect(groupsOf()).toBe(before);
  });

  it('Given a changed order and an edited group, When the store is reset, Then the groups are the defaults and the switch and the rail style are kept', () => {
    useSymbolBarStore.getState().setShown(false);
    useSymbolBarStore.getState().setRailStyle('symbols');
    useSymbolBarStore.getState().move(0, 7);
    useSymbolBarStore.getState().setGroup('g4', 'x');

    useSymbolBarStore.getState().reset();

    expect(isDefaultGroups(groupsOf())).toBe(true);
    expect(useSymbolBarStore.getState().shown).toBe(false);
    expect(useSymbolBarStore.getState().railStyle).toBe('symbols');
  });

  it('Given changes made, When the stored value is read back, Then it sits under leetcode-symbol-bar at version 1 with the three fields and no functions', async () => {
    useSymbolBarStore.getState().setShown(false);
    useSymbolBarStore.getState().move(1, 2);
    useSymbolBarStore.getState().setRailStyle('symbols');

    const raw = JSON.parse((await AsyncStorage.getItem(KEY)) ?? 'null');

    expect(raw.version).toBe(1);
    expect(Object.keys(raw.state).sort()).toEqual(['groups', 'railStyle', 'shown']);
    expect(raw.state.shown).toBe(false);
    expect(raw.state.railStyle).toBe('symbols');
    expect(raw.state.groups.map((group: { chars: string }) => group.chars).slice(0, 3)).toEqual(['#$^%', '\\@~_', '&|?!']);
  });
});
