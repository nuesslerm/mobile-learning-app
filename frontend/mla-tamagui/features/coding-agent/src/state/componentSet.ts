import { createContext, useContext } from 'react';

import type { ComponentSetPreference } from '../domain/ports';

export interface ComponentSetValue {
  preference: ComponentSetPreference;
  /** Picks a design system; whoever owns the preference persists it. */
  select: (preference: ComponentSetPreference) => void;
}

const ComponentSetContext = createContext<ComponentSetValue>({
  preference: 'tamagui',
  select: () => undefined,
});

export const ComponentSetProvider = ComponentSetContext.Provider;

export function useComponentSet(): ComponentSetValue {
  return useContext(ComponentSetContext);
}
