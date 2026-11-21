import { createContext, useContext, type ReactNode } from 'react';

import type { TamilWritingPorts } from '../domain/ports';

const TamilWritingPortsContext = createContext<TamilWritingPorts | null>(null);

export function TamilWritingPortsProvider({ value, children }: { value: TamilWritingPorts; children: ReactNode }) {
  return <TamilWritingPortsContext.Provider value={value}>{children}</TamilWritingPortsContext.Provider>;
}

export function useTamilWritingPorts(): TamilWritingPorts {
  const ports = useContext(TamilWritingPortsContext);
  if (!ports) throw new Error('TamilWritingPortsProvider missing');
  return ports;
}
