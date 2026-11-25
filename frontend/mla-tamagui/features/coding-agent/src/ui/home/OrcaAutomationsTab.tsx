import { useAgentComponents } from '@lib/agent-ui-contracts';
import { ScrollView } from 'react-native';

/**
 * The staged home's automations tab: the scheduled work of a desktop.
 *
 * Without a desktop there is no schedule to show, so the tab carries the
 * original's empty state.
 */
export function OrcaAutomationsTab() {
  const { HomeTabHeader, HomeEmptyState } = useAgentComponents();
  return (
    <>
      <HomeTabHeader title="Automations" />
      <ScrollView contentContainerStyle={{ paddingTop: 8, paddingHorizontal: 16 }} showsVerticalScrollIndicator={false}>
        <HomeEmptyState
          icon="automations"
          title="Schedule agent work"
          message="Pair a desktop to view and run automations."
          titleSize={17}
          bodySize={15}
        />
      </ScrollView>
    </>
  );
}
