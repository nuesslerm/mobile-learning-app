import { appConfig } from '@lib/app-config';
import { overrideKey, useDependencyOverrides } from '@lib/app-di';
import { useMemo, type ReactNode } from 'react';

import { createOrdsGoldenComments } from '../data/goldenScreenshotCommentsClient';
import { createOrdsHealthReports } from '../data/ordsClient';
import type { OrdsConnection, ServiceReportPorts } from '../domain/ports';
import { ServiceReportPortsProvider } from '../state/ports';

const DEFAULT_ORDS_USER = 'health_rpt';

export const SERVICE_REPORT_OVERRIDES = overrideKey<ServiceReportPorts>('service-report');

function ordsConnection(): OrdsConnection | undefined {
  const { healthReportsOrdsBaseUrl: baseUrl, healthReportsOrdsUser: user, healthReportsOrdsPassword: password } = appConfig;
  if (!baseUrl || !password) return undefined;
  return { baseUrl, user: user || DEFAULT_ORDS_USER, password };
}

export function createDefaultPorts(overrides: Partial<ServiceReportPorts> = {}): ServiceReportPorts {
  const connection = ordsConnection();
  return {
    healthReports: overrides.healthReports ?? createOrdsHealthReports(connection),
    goldenComments: overrides.goldenComments ?? createOrdsGoldenComments(connection),
  };
}

export function ServiceReportContainer({ children }: { children: ReactNode }) {
  const overrides = useDependencyOverrides(SERVICE_REPORT_OVERRIDES);
  const ports = useMemo(() => createDefaultPorts(overrides), [overrides]);
  return <ServiceReportPortsProvider value={ports}>{children}</ServiceReportPortsProvider>;
}
