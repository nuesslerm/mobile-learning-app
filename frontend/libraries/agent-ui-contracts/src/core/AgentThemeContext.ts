import { createContext, useContext } from 'react';
import type { AgentTheme } from '../ports/AgentTheme';

const AgentThemeContext = createContext<AgentTheme | null>(null);
export const AgentThemeProvider = AgentThemeContext.Provider;

export function useAgentTheme(): AgentTheme {
  const theme = useContext(AgentThemeContext);
  if (!theme) throw new Error('AgentThemeProvider is required');
  return theme;
}
