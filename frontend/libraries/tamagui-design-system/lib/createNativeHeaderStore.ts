import { Logger } from '@lib/logging';
import { create } from 'zustand';
import { createJSONStorage, persist, type StateStorage } from 'zustand/middleware';

export const DEFAULT_NATIVE_HEADERS = true;
export const NATIVE_HEADERS_STORAGE_KEY = 'native-headers';

export type NativeHeaderState = {
  enabled: boolean;
  setEnabled: (enabled: boolean) => void;
};

function enabledFrom(persisted: unknown): boolean {
  const value = (persisted as { enabled?: unknown } | undefined)?.enabled;
  return typeof value === 'boolean' ? value : DEFAULT_NATIVE_HEADERS;
}

/** The store over a storage handed in, so a test needs no native package; a stored value that is not a boolean falls back to on. */
export function createNativeHeaderStore(storage: StateStorage) {
  return create<NativeHeaderState>()(
    persist(
      (set) => ({
        enabled: DEFAULT_NATIVE_HEADERS,
        setEnabled: (enabled) => {
          Logger.info('nativeHeaders.set', { enabled });
          set({ enabled });
        },
      }),
      {
        name: NATIVE_HEADERS_STORAGE_KEY,
        version: 1,
        storage: createJSONStorage(() => storage),
        partialize: (state) => ({ enabled: state.enabled }),
        merge: (persisted, current) => ({ ...current, enabled: enabledFrom(persisted) }),
        onRehydrateStorage: () => (_state, err) => {
          if (err) Logger.withError(err).error('nativeHeaders.rehydrate_failed');
        },
      }
    )
  );
}
