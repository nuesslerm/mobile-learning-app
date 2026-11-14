import { createContext, useContext, type ReactNode } from 'react';

import type { CodingAgentPorts } from '../domain/ports';

const CodingAgentPortsContext = createContext<CodingAgentPorts | null>(null);

export function CodingAgentPortsProvider({ value, children }: { value: CodingAgentPorts; children: ReactNode }) {
  return <CodingAgentPortsContext.Provider value={value}>{children}</CodingAgentPortsContext.Provider>;
}

export function useCodingAgentPorts(): CodingAgentPorts {
  const ports = useContext(CodingAgentPortsContext);
  if (!ports) throw new Error('CodingAgentPortsProvider missing');
  return ports;
}
