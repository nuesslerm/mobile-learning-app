import { Directory, File, Paths } from 'expo-file-system';

import type { SnapshotStorage } from './snapshotStorage';

export function createDeviceStorage(): SnapshotStorage {
  const directory = new Directory(Paths.document, 'tamil-progress');
  return {
    async getItem(key) {
      const file = new File(directory, key);
      return file.exists ? file.text() : null;
    },
    async setItem(key, value) {
      directory.create({ idempotent: true, intermediates: true });
      await new File(directory, key).write(value);
    },
    async removeItem(key) {
      const file = new File(directory, key);
      if (file.exists) file.delete();
    },
  };
}
