import { useAgentComponents } from '@lib/agent-ui-contracts';
import { useEffect, type ReactNode } from 'react';
import { BackHandler, Platform, StyleSheet, View } from 'react-native';
import { SafeAreaView, useSafeAreaInsets } from 'react-native-safe-area-context';

import { useHomeNavigation } from '../state/useHomeNavigation';
import { OrcaAgentsTab } from './home/OrcaAgentsTab';
import { OrcaAutomationsTab } from './home/OrcaAutomationsTab';
import { router } from 'expo-router';
import { OrcaBotsTab } from './home/OrcaBotsTab';
import { OrcaUsageTab } from './home/OrcaUsageTab';

export function OrcaHome({ feed, computer }: { feed: ReactNode; computer: string | null | undefined }) {
  const { HomeSurface, HomeTabBar, HomeTabHeader } = useAgentComponents();
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
              <>
                <HomeTabHeader title="Feed" actions={[{
                  id: 'settings', icon: 'settings', accessibilityLabel: 'Settings',
                  onPress: () => router.push('/coding-agent/settings'),
                }]} />
                {feed}
              </>
            ) : entry.id === 'bots' ? (
              <OrcaBotsTab computer={computer} />
            ) : entry.id === 'agents' ? (
              <OrcaAgentsTab />
            ) : entry.id === 'usage' ? (
              <OrcaUsageTab />
            ) : (
              <OrcaAutomationsTab />
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
