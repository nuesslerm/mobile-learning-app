import AsyncStorage from '@react-native-async-storage/async-storage';
import { z } from 'zod';

import type { LibrarySnapshot, LibraryStoragePort } from '../../domain/library/library';

const schema = z.object({
  lastOpened: z.string().nullable(),
  lists: z.array(z.object({
    id: z.string(),
    name: z.string(),
    description: z.string(),
    private: z.boolean(),
    problemIds: z.array(z.string()),
    coverUri: z.string().optional(),
  })),
});

export function createDeviceLibrary(key: string, initial: LibrarySnapshot | null = null): LibraryStoragePort {
  return {
    async read() {
      const raw = await AsyncStorage.getItem(key);
      return raw === null ? initial : schema.parse(JSON.parse(raw));
    },
    async write(snapshot) {
      await AsyncStorage.setItem(key, JSON.stringify(snapshot));
    },
  };
}

export const deviceLibrary = createDeviceLibrary('leetcode-library-v1');
