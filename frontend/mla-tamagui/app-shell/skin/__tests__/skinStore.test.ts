import { describe, expect, it } from 'vitest';

import { createSkinStorage } from '../skinStore';

function memoryStorage(initial: Record<string, string> = {}) {
  const items = new Map(Object.entries(initial));
  return {
    items,
    getItem: async (key: string) => items.get(key) ?? null,
    setItem: async (key: string, value: string) => void items.set(key, value),
    removeItem: async (key: string) => void items.delete(key),
  };
}

const stored = (skin: string) => JSON.stringify({ state: { skin }, version: 0 });

describe('The storage behind the skin choice', () => {
  it('Given a device that never chose, when the store reads, then nothing is stored', async () => {
    const storage = createSkinStorage(memoryStorage());

    expect(await storage.getItem('skin-store')).toBeNull();
  });

  it('Given a choice an earlier build kept for the coding agent, when the store reads, then the choice is kept and moved', async () => {
    const backing = memoryStorage({ 'coding-agent.settings.componentSet': 'orca' });
    const storage = createSkinStorage(backing);

    expect(await storage.getItem('skin-store')).toBe(stored('orca'));
    expect(backing.items.get('skin-store')).toBe(stored('orca'));
    expect(backing.items.has('coding-agent.settings.componentSet')).toBe(false);
  });

  it('Given choices under both earlier names, when the store reads, then the newer name wins', async () => {
    const backing = memoryStorage({
      'coding-agent.settings.componentSet': 'orca',
      'coding-agent.settings.skin': 'tamagui',
    });

    expect(await createSkinStorage(backing).getItem('skin-store')).toBe(stored('tamagui'));
  });

  it('Given the store already has its own entry, when it reads, then an earlier choice is ignored', async () => {
    const backing = memoryStorage({ 'skin-store': stored('tamagui'), 'coding-agent.settings.skin': 'orca' });

    expect(await createSkinStorage(backing).getItem('skin-store')).toBe(stored('tamagui'));
  });
});

describe('A device whose storage cannot be read', () => {
  it('Given storage that throws, when the store reads, then it answers as if nothing was stored', async () => {
    const storage = createSkinStorage({
      getItem: async () => {
        throw new Error('storage unavailable');
      },
      setItem: async () => undefined,
      removeItem: async () => undefined,
    });

    expect(await storage.getItem('skin-store')).toBeNull();
  });
});
