import { Logger } from '@lib/logging';
import { useMemo } from 'react';
import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

import { keyValueStorage } from '../../lib/keyValueStore';
import {
  getDefaultFontPreset,
  getFontPresetById,
  type FontPresetConfig,
} from '../fontPresets';

type FontPresetState = {
  activeFontId: string | null;
  setActiveFont: (id: string | null) => void;
};

export const useFontPresetStore = create<FontPresetState>()(
  persist(
    (set) => ({
      activeFontId: null,
      setActiveFont: (id) => {
        Logger.info('font.setActive', {
          id,
        });
        set({
          activeFontId: id,
        });
      },
    }),
    {
      name: 'font-preset-store',
      storage: createJSONStorage(() => keyValueStorage),
      partialize: (s) => ({
        activeFontId: s.activeFontId,
      }),
      merge: (persisted, current) => {
        if (typeof persisted !== 'object' || persisted === null) {
          Logger.info('font.rehydrate.fallback');
          return current;
        }
        const p = persisted as Partial<FontPresetState>;
        return {
          ...current,
          activeFontId:
            typeof p.activeFontId === 'string'
              ? p.activeFontId
              : current.activeFontId,
        };
      },
    }
  )
);

/** The active font preset (name, description, face map); default when unset. */
export function getActiveFontPreset(
  activeFontId: string | null
): FontPresetConfig {
  return (
    (activeFontId ? getFontPresetById(activeFontId) : undefined) ??
    getDefaultFontPreset()
  );
}

/** The currently active font preset (name, description, face map). */
export function useActiveFontPreset(): FontPresetConfig {
  const activeFontId = useFontPresetStore((s) => s.activeFontId);
  return useMemo(() => getActiveFontPreset(activeFontId), [activeFontId]);
}
