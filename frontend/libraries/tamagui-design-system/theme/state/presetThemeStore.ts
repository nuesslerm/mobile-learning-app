import { Logger } from '@lib/logging';
import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

import { keyValueStorage } from '../../lib/keyValueStore';
import { getDefaultTheme, getPresetThemeById } from '../presetThemes';

export type ThemeMode = 'system' | 'light' | 'dark';

type PresetThemeState = {
  themeMode: ThemeMode;
  activeThemeId: string | null;
  setThemeMode: (mode: ThemeMode) => void;
  setActiveTheme: (id: string | null) => void;
};

export const usePresetThemeStore = create<PresetThemeState>()(
  persist(
    (set) => ({
      themeMode: 'system',
      activeThemeId: null,
      setThemeMode: (mode) => {
        Logger.info('theme.setMode', {
          mode,
        });
        set({
          themeMode: mode,
        });
      },
      setActiveTheme: (id) => {
        Logger.info('theme.setActive', {
          id,
        });
        set({
          activeThemeId: id,
        });
      },
    }),
    {
      name: 'preset-theme-store',
      storage: createJSONStorage(() => keyValueStorage),
      partialize: (s) => ({
        themeMode: s.themeMode,
        activeThemeId: s.activeThemeId,
      }),
      merge: (persisted, current) => {
        if (typeof persisted !== 'object' || persisted === null) {
          // Corrupt persisted state — fall back to defaults, which get
          // re-persisted on the next write.
          Logger.info('theme.rehydrate.fallback');
          return current;
        }
        const p = persisted as Partial<PresetThemeState>;
        return {
          ...current,
          themeMode:
            p.themeMode === 'light' ||
            p.themeMode === 'dark' ||
            p.themeMode === 'system'
              ? p.themeMode
              : current.themeMode,
          activeThemeId:
            typeof p.activeThemeId === 'string'
              ? p.activeThemeId
              : current.activeThemeId,
        };
      },
    }
  )
);

/**
 * Map the store state to a Tamagui theme name (see theme/themes.ts, keyed
 * `${presetId}_${mode}`). The OS scheme is passed in by the caller so this
 * stays pure and unit-testable; unknown/cleared ids resolve to the default
 * ios-native preset.
 */
export function resolveActiveThemeName(
  activeThemeId: string | null,
  themeMode: ThemeMode,
  systemScheme: 'light' | 'dark'
): string {
  const preset =
    (activeThemeId ? getPresetThemeById(activeThemeId) : undefined) ??
    getDefaultTheme();
  const mode = themeMode === 'system' ? systemScheme : themeMode;
  return `${preset.id}_${mode}`;
}
