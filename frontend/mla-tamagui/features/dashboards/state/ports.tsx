import { createContext, useContext, type ReactNode } from 'react';

import type { DashboardsPorts } from '../domain/ports';

const DashboardsPortsContext = createContext<DashboardsPorts | null>(null);

export function DashboardsPortsProvider({ value, children }: { value: DashboardsPorts; children: ReactNode }) {
  return <DashboardsPortsContext.Provider value={value}>{children}</DashboardsPortsContext.Provider>;
}

export function useDashboardsPorts(): DashboardsPorts {
  const ports = useContext(DashboardsPortsContext);
  if (!ports) throw new Error('DashboardsPortsProvider missing');
  return ports;
}
