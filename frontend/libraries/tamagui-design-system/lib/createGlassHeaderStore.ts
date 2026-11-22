import { Logger } from '@lib/logging';
import { create } from 'zustand';
import { createJSONStorage, persist, type StateStorage } from 'zustand/middleware';

export const DEFAULT_GLASS_HEADERS = false;
export const GLASS_HEADERS_STORAGE_KEY = 'glass-headers';

export type GlassHeaderState = {
  enabled: boolean;
  setEnabled: (enabled: boolean) => void;
};

function enabledFrom(persisted: unknown): boolean {
  const value = (persisted as { enabled?: unknown } | undefined)?.enabled;
  return typeof value === 'boolean' ? value : DEFAULT_GLASS_HEADERS;
}

/** The store over a storage handed in, so a test needs no native package; a stored value that is not a boolean falls back to off. */
export function createGlassHeaderStore(storage: StateStorage) {
  return create<GlassHeaderState>()(
    persist(
      (set) => ({
        enabled: DEFAULT_GLASS_HEADERS,
        setEnabled: (enabled) => {
          Logger.info('glassHeaders.set', { enabled });
          set({ enabled });
        },
      }),
      {
        name: GLASS_HEADERS_STORAGE_KEY,
        version: 1,
        storage: createJSONStorage(() => storage),
        partialize: (state) => ({ enabled: state.enabled }),
        merge: (persisted, current) => ({ ...current, enabled: enabledFrom(persisted) }),
        onRehydrateStorage: () => (_state, err) => {
          if (err) Logger.withError(err).error('glassHeaders.rehydrate_failed');
        },
      }
    )
  );
}
