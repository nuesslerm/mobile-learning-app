import { appConfig } from '@lib/app-config';
import { overrideKey, useDependencyOverrides } from '@lib/app-di';
import { useMemo, type ReactNode } from 'react';

import { createHttpWritingCheck } from '../data/httpWritingCheck';
import type { TamilWritingPorts } from '../domain/ports';
import { TamilWritingPortsProvider } from '../state/ports';

const DEFAULT_AGENT_BASE_URL = 'http://localhost:8787';

export const TAMIL_WRITING_OVERRIDES = overrideKey<TamilWritingPorts>('tamilWriting');

export function createDefaultPorts(overrides: Partial<TamilWritingPorts> = {}): TamilWritingPorts {
  return {
    writingCheck: overrides.writingCheck ?? createHttpWritingCheck(appConfig.agentBaseUrl ?? DEFAULT_AGENT_BASE_URL),
  };
}

export function TamilWritingContainer({ children }: { children: ReactNode }) {
  const overrides = useDependencyOverrides(TAMIL_WRITING_OVERRIDES);
  const ports = useMemo(() => createDefaultPorts(overrides), [overrides]);
  return <TamilWritingPortsProvider value={ports}>{children}</TamilWritingPortsProvider>;
}
