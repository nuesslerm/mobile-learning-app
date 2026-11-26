// The Orca home: the five tabs of the original, in its order, each with
// its own screen, and the bar that switches between them.
//
// Ported from `references/orca/mobile/src/home/MobileHomeScreen.tsx` at
// `980c0024741812bd7ae934c0c22144d9926a784e` (Orca, MIT, Lovecast Inc.).

import { useAgentComponents } from '@lib/agent-ui-contracts';
import { useEffect, type ReactNode } from 'react';
import { BackHandler, Platform, StyleSheet, View } from 'react-native';
import { SafeAreaView, useSafeAreaInsets } from 'react-native-safe-area-context';

import { useHomeNavigation } from '../state/useHomeNavigation';
import { OrcaAgentsTab } from './home/OrcaAgentsTab';
import { OrcaAutomationsTab } from './home/OrcaAutomationsTab';
import { OrcaSettingsTab } from './home/OrcaSettingsTab';
import { OrcaUsageTab } from './home/OrcaUsageTab';

export function OrcaHome({ feed }: { feed: ReactNode }) {
  const { HomeSurface, HomeTabBar } = useAgentComponents();
  const { bottom } = useSafeAreaInsets();
  const { entries, active, visited, select } = useHomeNavigation();

  // Android Back from another tab returns to Feed before it leaves the app.
  useEffect(() => {
    if (Platform.OS !== 'android' || active === 'feed') return undefined;
    const subscription = BackHandler.addEventListener('hardwareBackPress', () => {
      select('feed');
      return true;
    });
    return () => subscription.remove();
  }, [active, select]);

  return (
    <HomeSurface><SafeAreaView style={styles.screen} edges={['top']} testID="OrcaHome">
      {entries.map((entry) =>
        visited.has(entry.id) ? (
          <View
            key={entry.id}
            style={entry.id === active ? styles.fill : styles.hidden}
          >
            {entry.id === 'feed' ? (
              feed
            ) : entry.id === 'agents' ? (
              <OrcaAgentsTab />
            ) : entry.id === 'usage' ? (
              <OrcaUsageTab />
            ) : entry.id === 'automations' ? (
              <OrcaAutomationsTab />
            ) : (
              <OrcaSettingsTab />
            )}
          </View>
        ) : null
      )}
      <HomeTabBar
        tabs={entries.map((entry) => ({ id: entry.id, label: entry.label }))}
        active={active}
        bottomInset={bottom}
        onSelect={select}
      />
    </SafeAreaView></HomeSurface>
  );
}

const styles = StyleSheet.create({
  screen: { flex: 1 },
  fill: { flex: 1 },
  hidden: { display: 'none' },
});
