// The frame the coding agent's settings screens share: the safe area, then the
// bar with the screen's title and its rows.

import { useAgentComponents } from '@lib/agent-ui-contracts';
import { homePalettes, useOrcaScheme } from '@lib/orca-design-system';
import { router } from 'expo-router';
import type { ReactNode } from 'react';
import { View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

export function OrcaSettingsScreen(props: { title: string; children: ReactNode }) {
  const { HomeSettingsScreen } = useAgentComponents();
  const colors = homePalettes[useOrcaScheme()];
  const { top } = useSafeAreaInsets();
  return (
    <View style={{ flex: 1, paddingTop: top, backgroundColor: colors.bgBase }}>
      <HomeSettingsScreen title={props.title} onBack={() => router.canGoBack() ? router.back() : router.replace('/coding-agent')}>
        {props.children}
      </HomeSettingsScreen>
    </View>
  );
}
