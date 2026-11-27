import { createContext, createElement, useContext, type ComponentType, type ReactNode } from 'react';

/** Which skin draws the app. */
export type SkinPreference = 'tamagui' | 'orca';

export interface SkinPreferenceValue {
  preference: SkinPreference;
  /** Picks a skin; whoever owns the preference persists it. */
  select: (preference: SkinPreference) => void;
}

const SkinPreferenceContext = createContext<SkinPreferenceValue>({
  preference: 'tamagui',
  select: () => undefined,
});

export const SkinPreferenceProvider = SkinPreferenceContext.Provider;

export function useSkinPreference(): SkinPreferenceValue {
  return useContext(SkinPreferenceContext);
}

type ThemeComponent = ComponentType<{ children: ReactNode }>;

const ActiveSkinThemeContext = createContext<ThemeComponent | null | undefined>(undefined);

/**
 * The composition root hands over the active skin's theme provider; a subtree mounts it where its appearance is known.
 * `null` while the stored choice is still being read: the subtree waits, so it never mounts under the wrong skin.
 */
export const ActiveSkinThemeProvider = ActiveSkinThemeContext.Provider;

export function SkinThemeBoundary({ children }: { children: ReactNode }) {
  const Theme = useContext(ActiveSkinThemeContext);
  if (Theme === undefined) throw new Error('ActiveSkinThemeProvider is required');
  if (Theme === null) return null;
  return createElement(Theme, null, children);
}
