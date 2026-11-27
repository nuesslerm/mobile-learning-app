import { createContext, useContext } from 'react';

import type { SkinPreference } from '../domain/ports';

export interface SkinPreferenceValue {
  preference: SkinPreference;
  /** Picks a design system; whoever owns the preference persists it. */
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
