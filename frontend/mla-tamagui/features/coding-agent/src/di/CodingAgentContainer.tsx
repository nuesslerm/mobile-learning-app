import { createWorkspaceTabsApi } from '../data/t3/workspaceTabs';
import { ChatTextGate } from './ChatTextGate';
import { imageActionsFor } from '../data/imageActions';
import { copyMessage } from '../data/copyMessage';
import { pairingCamera } from '../data/pairingCamera';
import { createT3MobileApi } from '../data/t3/mobileApi';
import {
  AppearancePreferenceContextProvider,
  SkinProvider,
  SkinThemeBoundary,
  useSkin,
  type Skin,
} from '@lib/skin';
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
  overrideKey<Skin>('coding-agent-ui');

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

/** A device choice: read once, then kept in step with what the user picks. */
function useStoredPreference<T>(
  initial: T,
  read: () => Promise<T>,
  write: (next: T) => Promise<void>
): { preference: T; select: (next: T) => void } {
  const [preference, setPreference] = useState(initial);

  useEffect(() => {
    let active = true;
    void read().then((stored) => {
      if (active) {
        setPreference(stored);
      }
    });
    return () => {
      active = false;
    };
  }, [read]);

  const select = useCallback(
    (next: T) => {
      setPreference(next);
      void write(next);
    },
    [write]
  );

  return { preference, select };
}

/** Hands the theme choice to the skin, once the ports are in place. */
function AppearanceGate({ children }: { children: ReactNode }) {
  const { preferences } = useCodingAgentPorts();
  const appearance = useStoredPreference<AppearancePreference>(
    'system',
    preferences.readAppearance,
    preferences.writeAppearance
  );
  return (
    <AppearancePreferenceContextProvider value={appearance}>
      <SkinThemeBoundary>{children}</SkinThemeBoundary>
    </AppearancePreferenceContextProvider>
  );
}

export function CodingAgentContainer({ children }: { children: ReactNode }) {
  const overrides = useDependencyOverrides(CODING_AGENT_OVERRIDES);
  const ports = useMemo(() => createDefaultPorts(overrides), [overrides]);
  return (
    <CodingAgentPortsProvider value={ports}>
      <AppearanceGate>
        <ChatTextGate>
          <SkinGate>{children}</SkinGate>
        </ChatTextGate>
      </AppearanceGate>
    </CodingAgentPortsProvider>
  );
}

/** The app's skin, with the entries a test replaces. */
function SkinGate({ children }: { children: ReactNode }) {
  const skin = useSkin();
  const overrides = useDependencyOverrides(CODING_AGENT_UI_OVERRIDES);
  const withOverrides = useMemo(() => ({ ...skin, ...overrides }), [skin, overrides]);
  return <SkinProvider value={withOverrides}>{children}</SkinProvider>;
}
