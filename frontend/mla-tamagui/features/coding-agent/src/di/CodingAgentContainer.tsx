import { createWorkspaceTabsApi } from '../data/t3/workspaceTabs';
import { ChatTextGate } from './ChatTextGate';
import { imageActionsFor } from '../data/imageActions';
import { copyMessage } from '../data/copyMessage';
import { pairingCamera } from '../data/pairingCamera';
import { createT3MobileApi } from '../data/t3/mobileApi';
import {
  AgentComponentsProvider,
  AppearancePreferenceContextProvider,
  type AgentComponents,
} from '@lib/agent-ui-contracts';
import { OrcaThemeProvider, orcaComponents } from '@lib/orca-design-system';
import { appConfig } from '@lib/app-config';
import { overrideKey, useDependencyOverrides } from '@lib/app-di';
import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';

import { createExpoConstantsAppInfo } from '../data/appInfo';
import { createExpoUpdatesAppUpdate } from '../data/appUpdate';
import { createLinkingExternalLinks } from '../data/externalLinks';
import { createHttpDispatchRunsPort } from '../data/dispatchRunsGateway';
import { createAsyncStorageSettingsPreferences } from '../data/settingsPreferences';
import { createAsyncStorageTokenStore } from '../data/tokenStore';
import { createMobileApi } from '../data/rca/mobileApi';
import { createWsOrchestration } from '../data/wsOrchestration';
import type { AppearancePreference, CodingAgentPorts } from '../domain/ports';
import { CodingAgentPortsProvider, useCodingAgentPorts } from '../state/ports';

export const CODING_AGENT_OVERRIDES =
  overrideKey<CodingAgentPorts>('coding-agent');

export const CODING_AGENT_UI_OVERRIDES =
  overrideKey<AgentComponents>('coding-agent-ui');

const devicePreferences = createAsyncStorageSettingsPreferences();

export function createDefaultPorts(
  overrides: Partial<CodingAgentPorts> = {}
): CodingAgentPorts {
  const tokenStore = overrides.tokenStore ?? createAsyncStorageTokenStore();
  return {
    tokenStore,
    imageActionsFor: overrides.imageActionsFor ?? imageActionsFor,
    copyMessage: overrides.copyMessage ?? copyMessage,
    pairingCamera: overrides.pairingCamera ?? pairingCamera,
    workspaceTabs: overrides.workspaceTabs ?? (overrides.t3Mobile ? undefined : createWorkspaceTabsApi()),
    t3Mobile: overrides.t3Mobile ?? createT3MobileApi(),
    mobile:
      overrides.mobile ??
      (overrides.orchestration ? undefined : createMobileApi()),
    orchestration: overrides.orchestration ?? createWsOrchestration(tokenStore),
    dispatchRuns:
      overrides.dispatchRuns ??
      createHttpDispatchRunsPort(appConfig.vmOpsApiBaseUrl),
    preferences: overrides.preferences ?? devicePreferences,
    appUpdate: overrides.appUpdate ?? createExpoUpdatesAppUpdate(),
    appInfo: overrides.appInfo ?? createExpoConstantsAppInfo(),
    externalLinks: overrides.externalLinks ?? createLinkingExternalLinks(),
  };
}

/** The device's theme choice: read once, then kept in step with what the user picks. */
function useAppearancePreferenceState(): {
  preference: AppearancePreference;
  select: (preference: AppearancePreference) => void;
} {
  const { preferences } = useCodingAgentPorts();
  const [preference, setPreference] = useState<AppearancePreference>('system');

  useEffect(() => {
    let active = true;
    void preferences.readAppearance().then((stored) => {
      if (active) {
        setPreference(stored);
      }
    });
    return () => {
      active = false;
    };
  }, [preferences]);

  const select = useCallback(
    (next: AppearancePreference) => {
      setPreference(next);
      void preferences.writeAppearance(next);
    },
    [preferences]
  );

  return { preference, select };
}

/** Hands the theme choice to the design system, once the ports are in place. */
function AppearanceGate({ children }: { children: ReactNode }) {
  const appearance = useAppearancePreferenceState();
  return (
    <AppearancePreferenceContextProvider
      value={{ preference: appearance.preference, select: appearance.select }}
    >
      <OrcaThemeProvider>{children}</OrcaThemeProvider>
    </AppearancePreferenceContextProvider>
  );
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
      <AppearanceGate>
        <ChatTextGate>
          <AgentComponentsProvider value={components}>
            {children}
          </AgentComponentsProvider>
        </ChatTextGate>
      </AppearanceGate>
    </CodingAgentPortsProvider>
  );
}
