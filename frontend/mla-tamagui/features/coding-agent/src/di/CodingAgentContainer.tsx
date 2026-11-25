import { pairingCamera } from '../data/pairingCamera';
import {
  AgentComponentsProvider,
  type AgentComponents,
} from '@lib/agent-ui-contracts';
import { orcaComponents } from '@lib/orca-design-system';
import { appConfig } from '@lib/app-config';
import { overrideKey, useDependencyOverrides } from '@lib/app-di';
import { useMemo, type ReactNode } from 'react';

import { createHttpDispatchRunsPort } from '../data/dispatchRunsGateway';
import { createAsyncStorageTokenStore } from '../data/tokenStore';
import { createT3MobileApi } from '../data/t3/mobileApi';
import { createMobileApi } from '../data/rca/mobileApi';
import { createWsOrchestration } from '../data/wsOrchestration';
import type { CodingAgentPorts } from '../domain/ports';
import { CodingAgentPortsProvider } from '../state/ports';

export const CODING_AGENT_OVERRIDES =
  overrideKey<CodingAgentPorts>('coding-agent');

export const CODING_AGENT_UI_OVERRIDES =
  overrideKey<AgentComponents>('coding-agent-ui');

export function createDefaultPorts(
  overrides: Partial<CodingAgentPorts> = {}
): CodingAgentPorts {
  const tokenStore = overrides.tokenStore ?? createAsyncStorageTokenStore();
  return {
    tokenStore,
    pairingCamera: overrides.pairingCamera ?? pairingCamera,
    t3Mobile: overrides.t3Mobile ?? createT3MobileApi(),
    mobile:
      overrides.mobile ??
      (overrides.orchestration ? undefined : createMobileApi()),
    orchestration: overrides.orchestration ?? createWsOrchestration(tokenStore),
    dispatchRuns:
      overrides.dispatchRuns ??
      createHttpDispatchRunsPort(appConfig.vmOpsApiBaseUrl),
  };
}

export function CodingAgentContainer({ children }: { children: ReactNode }) {
  const overrides = useDependencyOverrides(CODING_AGENT_OVERRIDES);
  const ports = useMemo(() => createDefaultPorts(overrides), [overrides]);
  const uiOverrides = useDependencyOverrides(CODING_AGENT_UI_OVERRIDES);
  const components = useMemo(
    () => ({ ...orcaComponents, ...uiOverrides }),
    [uiOverrides]
  );
  return (
    <CodingAgentPortsProvider value={ports}>
      <AgentComponentsProvider value={components}>
        {children}
      </AgentComponentsProvider>
    </CodingAgentPortsProvider>
  );
}
