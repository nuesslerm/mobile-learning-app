import { createContext, useContext, type ReactNode } from 'react';

import type { LeetCodePorts } from '../domain/ports';

const LeetCodePortsContext = createContext<LeetCodePorts | null>(null);

export function LeetCodeContainerProvider({
  value,
  children,
}: {
  value: LeetCodePorts;
  children: ReactNode;
}) {
  return <LeetCodePortsContext.Provider value={value}>{children}</LeetCodePortsContext.Provider>;
}

export function useLeetCodePorts(): LeetCodePorts {
  const ports = useContext(LeetCodePortsContext);
  if (!ports) {
    throw new Error('LeetCodeContainerProvider missing');
  }
  return ports;
}
