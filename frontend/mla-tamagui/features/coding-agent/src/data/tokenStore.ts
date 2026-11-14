import AsyncStorage from '@react-native-async-storage/async-storage';
import { Logger } from '@lib/logging';

import type { TokenStorePort } from '../domain/ports';

export const CODING_AGENT_TOKEN_STORAGE_KEY = 'coding-agent.token';

type KeyValueStorage = Pick<typeof AsyncStorage, 'getItem' | 'setItem' | 'removeItem'>;

/** The bearer the local backend issued, kept in AsyncStorage; a missing token is not an error (the server may run with auth off). */
export function createAsyncStorageTokenStore(storage: KeyValueStorage = AsyncStorage): TokenStorePort {
  return {
    async read() {
      try {
        const value = await storage.getItem(CODING_AGENT_TOKEN_STORAGE_KEY);
        return value !== null && value.length > 0 ? value : null;
      } catch (cause) {
        Logger.withError(cause).withMetadata({ key: CODING_AGENT_TOKEN_STORAGE_KEY }).warn('coding-agent.config.token_read_failed');
        return null;
      }
    },
    async write(token) {
      await storage.setItem(CODING_AGENT_TOKEN_STORAGE_KEY, token);
    },
    async clear() {
      await storage.removeItem(CODING_AGENT_TOKEN_STORAGE_KEY);
    },
  };
}
