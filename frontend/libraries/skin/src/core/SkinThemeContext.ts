import { createContext, useContext } from 'react';
import type { SkinTheme } from '../ports/SkinTheme';

const SkinThemeContext = createContext<SkinTheme | null>(null);
export const SkinThemeProvider = SkinThemeContext.Provider;

export function useSkinTheme(): SkinTheme {
  const theme = useContext(SkinThemeContext);
  if (!theme) throw new Error('SkinThemeProvider is required');
  return theme;
}
