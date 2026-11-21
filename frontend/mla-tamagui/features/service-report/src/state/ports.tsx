import { createContext, useContext, type ReactNode } from 'react';

import type { ServiceReportPorts } from '../domain/ports';

const ServiceReportPortsContext = createContext<ServiceReportPorts | null>(null);

export function ServiceReportPortsProvider({ value, children }: { value: ServiceReportPorts; children: ReactNode }) {
  return <ServiceReportPortsContext.Provider value={value}>{children}</ServiceReportPortsContext.Provider>;
}

export function useServiceReportPorts(): ServiceReportPorts {
  const ports = useContext(ServiceReportPortsContext);
  if (!ports) throw new Error('ServiceReportPortsProvider missing');
  return ports;
}
