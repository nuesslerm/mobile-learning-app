import {
  AgentThemeProvider,
  useAppearancePreference,
} from '@lib/agent-ui-contracts';
import { Theme, useTheme } from '@lib/tamagui-design-system/primitives';
import {
  resolveActiveThemeName,
  usePresetThemeStore,
} from '@lib/tamagui-design-system/theme';
import { useMemo, type ReactNode } from 'react';
import { useColorScheme } from 'react-native';

import { agentThemeFrom } from '../../core/agentTheme';

function AgentThemeBridge({ scheme, children }: { scheme: 'light' | 'dark'; children: ReactNode }) {
  const theme = useTheme();
  const value = useMemo(
    () => agentThemeFrom((name) => theme[name]?.val as string | undefined, scheme),
    [theme, scheme]
  );
  return <AgentThemeProvider value={value}>{children}</AgentThemeProvider>;
}

/** The app's own theme, in the light or dark mode the agent's Appearance setting asks for. */
export function TamaguiThemeProvider({ children }: { children: ReactNode }) {
  const { preference } = useAppearancePreference();
  const activeThemeId = usePresetThemeStore((state) => state.activeThemeId);
  const themeMode = usePresetThemeStore((state) => state.themeMode);
  const system = useColorScheme() === 'dark' ? 'dark' : 'light';
  const name = resolveActiveThemeName(
    activeThemeId,
    preference === 'system' ? themeMode : preference,
    system
  );
  return (
    <Theme name={name}>
      <AgentThemeBridge scheme={name.endsWith('_dark') ? 'dark' : 'light'}>
        {children}
      </AgentThemeBridge>
    </Theme>
  );
}
