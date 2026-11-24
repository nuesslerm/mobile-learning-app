import AsyncStorage from '@react-native-async-storage/async-storage';

import type { SnapshotStorage } from './snapshotStorage';

export function createWritingStorage(storage: SnapshotStorage, legacy: SnapshotStorage = AsyncStorage): SnapshotStorage {
  return {
    async getItem(key) {
      const saved = await storage.getItem(key);
      if (saved !== null) return saved;

      const previous = await legacy.getItem(key);
      if (previous !== null) {
        await storage.setItem(key, previous);
        await legacy.removeItem(key);
      }
      return previous;
    },
    setItem: (key, value) => storage.setItem(key, value),
    async removeItem(key) {
      await storage.removeItem(key);
      await legacy.removeItem(key);
    },
  };
}
