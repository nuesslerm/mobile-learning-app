import { Logger } from '@lib/logging';
import { z } from 'zod';

export type SnapshotStorage = {
  getItem(key: string): Promise<string | null>;
  setItem(key: string, value: string): Promise<void>;
  removeItem(key: string): Promise<void>;
};

const snapshot = z.object({ generation: z.number().int().nonnegative(), value: z.string().nullable() });

export function createSnapshotStorage(files: SnapshotStorage): SnapshotStorage {
  let queue: Promise<unknown> = Promise.resolve();

  async function latest(key: string) {
    const slots = await Promise.all([0, 1].map(async (slot) => {
      const raw = await files.getItem(`${key}.${slot}`);
      if (!raw) return undefined;
      try {
        return snapshot.parse(JSON.parse(raw));
      } catch (error) {
        Logger.withError(error).warn('tamil.progress.snapshot_invalid');
        return undefined;
      }
    }));
    return slots[0] && (!slots[1] || slots[0].generation > slots[1].generation) ? slots[0] : slots[1];
  }

  function save(key: string, value: string | null) {
    const pending = queue.then(async () => {
      const generation = ((await latest(key))?.generation ?? -1) + 1;
      await files.setItem(`${key}.${generation % 2}`, JSON.stringify({ generation, value }));
    });
    queue = pending.catch(() => undefined);
    return pending;
  }

  return {
    async getItem(key) {
      await queue;
      return (await latest(key))?.value ?? null;
    },
    setItem: save,
    removeItem: (key) => save(key, null),
  };
}
