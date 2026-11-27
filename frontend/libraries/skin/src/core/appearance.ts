import { createContext, useContext } from 'react';

/** The theme a device asks for; "system" follows the device's own setting. */
export type AppearancePreference = 'system' | 'light' | 'dark';

export interface AppearancePreferenceValue {
  preference: AppearancePreference;
  /** Picks a theme; whoever owns the preference persists it. */
  select: (preference: AppearancePreference) => void;
}

const AppearancePreferenceContext = createContext<AppearancePreferenceValue>({
  preference: 'system',
  select: () => undefined,
});

export const AppearancePreferenceContextProvider = AppearancePreferenceContext.Provider;

/** The device's theme choice, or the default when nobody owns one. */
export function useAppearancePreference(): AppearancePreferenceValue {
  return useContext(AppearancePreferenceContext);
}
