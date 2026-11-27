import { createWorkspaceTabsApi } from '../data/t3/workspaceTabs';
import { ChatTextGate } from './ChatTextGate';
import { imageActionsFor } from '../data/imageActions';
import { copyMessage } from '../data/copyMessage';
import { pairingCamera } from '../data/pairingCamera';
import { createT3MobileApi } from '../data/t3/mobileApi';
import {
  SkinProvider,
  AppearancePreferenceContextProvider,
  type Skin,
} from '@lib/skin';
import { OrcaThemeProvider, orcaSkin } from '@lib/orca-skin';
import {
  TamaguiThemeProvider,
  tamaguiSkin,
} from '@lib/tamagui-skin';
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
import type {
  AppearancePreference,
  CodingAgentPorts,
  SkinPreference,
} from '../domain/ports';
import { SkinPreferenceProvider, useSkinPreference } from '../state/skinPreference';
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

const SKINS = {
  tamagui: { components: tamaguiSkin, Theme: TamaguiThemeProvider },
  orca: { components: orcaSkin, Theme: OrcaThemeProvider },
};

/** A device choice: read once, then kept in step with what the user picks. */
function useStoredPreference<T>(
  initial: T,
  read: () => Promise<T>,
  write: (next: T) => Promise<void>
): { preference: T; loaded: boolean; select: (next: T) => void } {
  const [preference, setPreference] = useState(initial);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let active = true;
    void read().then((stored) => {
      if (active) {
        setPreference(stored);
        setLoaded(true);
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

  return { preference, loaded, select };
}

/** Hands the theme and design choices to the screens, once the ports are in place. */
function AppearanceGate({ children }: { children: ReactNode }) {
  const { preferences } = useCodingAgentPorts();
  const appearance = useStoredPreference<AppearancePreference>(
    'system',
    preferences.readAppearance,
    preferences.writeAppearance
  );
  const skin = useStoredPreference<SkinPreference>(
    'tamagui',
    preferences.readSkin,
    preferences.writeSkin
  );
  return (
    <AppearancePreferenceContextProvider value={appearance}>
      <SkinPreferenceProvider value={skin}>
        {skin.loaded ? children : null}
      </SkinPreferenceProvider>
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

function SkinGate({ children }: { children: ReactNode }) {
  const { preference } = useSkinPreference();
  const { components: base, Theme } = SKINS[preference];
  const uiOverrides = useDependencyOverrides(CODING_AGENT_UI_OVERRIDES);
  const components = useMemo(
    () => ({ ...base, ...uiOverrides }),
    [base, uiOverrides]
  );
  return (
    <Theme>
      <SkinProvider value={components}>
        {children}
      </SkinProvider>
    </Theme>
  );
}
