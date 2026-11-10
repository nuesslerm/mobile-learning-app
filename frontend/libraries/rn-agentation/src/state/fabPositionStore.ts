// fabPositionStore.ts
import { create } from 'zustand';
import { createJSONStorage, persist, type StateStorage } from 'zustand/middleware';

import { clamp01 } from '../geometry/fabGeometry';

declare const require: (id: string) => any;

// AsyncStorage is an optional peer. If it isn't installed, fall back to an
// in-memory store so the FAB still drags (position just won't survive reload).
function resolveStorage(): StateStorage {
  try {
    // eslint-disable-next-line @typescript-eslint/no-var-requires
    const mod = require('@react-native-async-storage/async-storage');
    return (mod.default ?? mod) as StateStorage;
  } catch {
    const mem = new Map<string, string>();
    return {
      getItem: (k) => Promise.resolve(mem.get(k) ?? null),
      setItem: (k, v) => {
        mem.set(k, v);
        return Promise.resolve();
      },
      removeItem: (k) => {
        mem.delete(k);
        return Promise.resolve();
      },
    };
  }
}

interface FabPositionState {
  xRatio: number;
  yRatio: number;
  hydrated: boolean;
  setPosition: (xRatio: number, yRatio: number) => void;
  setHydrated: () => void;
}

// Default ≈ bottom-left, matching the current fixed placement.
const DEFAULT_X_RATIO = 0;
const DEFAULT_Y_RATIO = 0.82;

export const useFabPositionStore = create<FabPositionState>()(
  persist(
    (set) => ({
      xRatio: DEFAULT_X_RATIO,
      yRatio: DEFAULT_Y_RATIO,
      hydrated: false,
      setPosition: (xRatio, yRatio) =>
        set({ xRatio: clamp01(xRatio), yRatio: clamp01(yRatio) }),
      setHydrated: () => set({ hydrated: true }),
    }),
    {
      name: 'rn-agentation-fab-position',
      version: 1,
      storage: createJSONStorage(resolveStorage),
      partialize: (s) => ({ xRatio: s.xRatio, yRatio: s.yRatio }),
      onRehydrateStorage: () => (state) => state?.setHydrated(),
    },
  ),
);
