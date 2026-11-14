import { appConfig } from '@lib/app-config';
import { overrideKey, useDependencyOverrides } from '@lib/app-di';
import { useMemo, type ReactNode } from 'react';

import { createHttpReportingGateway } from '../data/reportingGateway';
import type { DashboardsPorts } from '../domain/ports';
import { DashboardsPortsProvider } from '../state/ports';

export const DASHBOARDS_OVERRIDES = overrideKey<DashboardsPorts>('dashboards');

export function createDefaultPorts(overrides: Partial<DashboardsPorts> = {}): DashboardsPorts {
  return {
    reportingGateway: overrides.reportingGateway ?? createHttpReportingGateway(appConfig.reportingBaseUrl),
  };
}

export function DashboardsContainer({ children }: { children: ReactNode }) {
  const overrides = useDependencyOverrides(DASHBOARDS_OVERRIDES);
  const ports = useMemo(() => createDefaultPorts(overrides), [overrides]);
  return <DashboardsPortsProvider value={ports}>{children}</DashboardsPortsProvider>;
}
