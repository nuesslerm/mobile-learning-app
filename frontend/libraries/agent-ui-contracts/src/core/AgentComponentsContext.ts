import { createContext, useContext } from 'react';
import type { AgentComponents } from '../ports/AgentComponents';

const AgentComponentsContext = createContext<AgentComponents | null>(null);
export const AgentComponentsProvider = AgentComponentsContext.Provider;

export function useAgentComponents(): AgentComponents {
  const components = useContext(AgentComponentsContext);
  if (!components) throw new Error('AgentComponentsProvider is required');
  return components;
}
