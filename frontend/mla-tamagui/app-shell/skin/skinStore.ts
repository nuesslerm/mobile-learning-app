import { Logger } from '@lib/logging';
import type { SkinPreference } from '@lib/skin';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { create } from 'zustand';
import { createJSONStorage, persist, type StateStorage } from 'zustand/middleware';

// Where the choice lived while only the coding agent had skins, newest first.
const LEGACY_KEYS = ['coding-agent.settings.skin', 'coding-agent.settings.componentSet'];

const asSkin = (value: unknown): SkinPreference => (value === 'orca' ? 'orca' : 'tamagui');

/** The store's storage: its own entry, or on first read the choice an earlier build kept under another key. */
export function createSkinStorage(storage: Pick<typeof AsyncStorage, 'getItem' | 'setItem' | 'removeItem'>): StateStorage {
  return {
    async getItem(name) {
      try {
        const own = await storage.getItem(name);
        if (own !== null) return own;
        for (const key of LEGACY_KEYS) {
          const legacy = await storage.getItem(key);
          if (legacy === null) continue;
          const migrated = JSON.stringify({ state: { skin: asSkin(legacy) }, version: 0 });
          await storage.setItem(name, migrated);
          await Promise.all(LEGACY_KEYS.map((old) => storage.removeItem(old)));
          return migrated;
        }
      } catch (cause) {
        // A read that throws would leave the store unhydrated and the skinned screens waiting forever.
        Logger.withError(cause).warn('skin.read_failed');
      }
      return null;
    },
    setItem: (name, value) => storage.setItem(name, value),
    removeItem: (name) => storage.removeItem(name),
  };
}

type SkinState = {
  skin: SkinPreference;
  setSkin: (skin: SkinPreference) => void;
};

export const useSkinStore = create<SkinState>()(
  persist(
    (set) => ({
      skin: 'tamagui',
      setSkin: (skin) => {
        Logger.info('skin.set', { skin });
        set({ skin });
      },
    }),
    {
      name: 'skin-store',
      storage: createJSONStorage(() => createSkinStorage(AsyncStorage)),
      partialize: (state) => ({ skin: state.skin }),
      merge: (persisted, current) => ({
        ...current,
        skin: asSkin((persisted as Partial<SkinState> | null)?.skin),
      }),
    }
  )
);
