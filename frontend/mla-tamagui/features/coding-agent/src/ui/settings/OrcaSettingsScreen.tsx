// The frame the coding agent's settings screens share: the safe area, then the
// bar with the screen's title and its rows.

import { useAgentComponents } from '@lib/agent-ui-contracts';
import { homePalettes, useOrcaScheme } from '@lib/orca-design-system';
import { router } from 'expo-router';
import type { ReactNode } from 'react';
import { SafeAreaView } from 'react-native-safe-area-context';

export function OrcaSettingsScreen(props: { title: string; children: ReactNode }) {
  const { HomeSettingsScreen } = useAgentComponents();
  const colors = homePalettes[useOrcaScheme()];
  return (
    <SafeAreaView edges={['top']} style={{ flex: 1, backgroundColor: colors.bgBase }}>
      <HomeSettingsScreen title={props.title} onBack={() => router.back()}>
        {props.children}
      </HomeSettingsScreen>
    </SafeAreaView>
  );
}
