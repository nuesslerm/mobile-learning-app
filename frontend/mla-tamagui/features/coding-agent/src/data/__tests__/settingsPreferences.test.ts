import { describe, expect, it } from 'vitest';

import { CODING_AGENT_SETTINGS_STORAGE_KEY, createAsyncStorageSettingsPreferences } from '../settingsPreferences';

function memoryStorage(initial: Record<string, string> = {}) {
  const items = new Map(Object.entries(initial));
  return {
    items,
    getItem: async (key: string) => items.get(key) ?? null,
    setItem: async (key: string, value: string) => void items.set(key, value),
    removeItem: async (key: string) => void items.delete(key),
  };
}

const key = (name: string) => `${CODING_AGENT_SETTINGS_STORAGE_KEY}.${name}`;

describe('The stored skin choice', () => {
  it('Given a device that never chose, when the skin is read, then it is the Tamagui skin', async () => {
    const preferences = createAsyncStorageSettingsPreferences(memoryStorage());

    expect(await preferences.readSkin()).toBe('tamagui');
  });

  it('Given a choice stored under the old name, when the skin is read, then the choice is kept and moved to the new name', async () => {
    const storage = memoryStorage({ [key('componentSet')]: 'orca' });
    const preferences = createAsyncStorageSettingsPreferences(storage);

    expect(await preferences.readSkin()).toBe('orca');
    expect(storage.items.get(key('skin'))).toBe('orca');
    expect(storage.items.has(key('componentSet'))).toBe(false);
  });

  it('Given choices under both names, when the skin is read, then the new name wins', async () => {
    const storage = memoryStorage({ [key('componentSet')]: 'orca', [key('skin')]: 'tamagui' });
    const preferences = createAsyncStorageSettingsPreferences(storage);

    expect(await preferences.readSkin()).toBe('tamagui');
  });

  it('Given a written choice, when the skin is read again, then it is that choice', async () => {
    const preferences = createAsyncStorageSettingsPreferences(memoryStorage());

    await preferences.writeSkin('orca');

    expect(await preferences.readSkin()).toBe('orca');
  });
});
