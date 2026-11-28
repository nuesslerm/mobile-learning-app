import { createContext, useContext } from 'react';
import type { Skin } from '../ports/Skin';

const SkinContext = createContext<Skin | null>(null);
export const SkinProvider = SkinContext.Provider;

export function useSkin(): Skin {
  const components = useContext(SkinContext);
  if (!components) throw new Error('SkinProvider is required');
  return components;
}
