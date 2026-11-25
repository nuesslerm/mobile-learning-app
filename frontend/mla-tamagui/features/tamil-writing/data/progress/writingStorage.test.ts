import { describe, expect, it } from 'vitest';

import { createMemoryTamilStorage } from '../../testing';
import { createWritingStorage } from './writingStorage';

describe('Tamil handwriting storage', () => {
  it('Given progress from the old store, When read twice, Then it is migrated once and retained', async () => {
    const files = createMemoryTamilStorage();
    const legacy = createMemoryTamilStorage();
    const key = 'tamil-writing-progress';
    const old = JSON.stringify({ version: 1, state: { characters: {}, totalAttempts: 9 } });
    await legacy.setItem(key, old);

    const adapter = createWritingStorage(files, legacy);
    expect(await adapter.getItem(key)).toBe(old);
    expect(await legacy.getItem(key)).toBeNull();
    expect(await createWritingStorage(files, legacy).getItem(key)).toBe(old);
  });
});
