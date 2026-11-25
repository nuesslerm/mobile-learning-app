import { useAgentComponents } from '@lib/agent-ui-contracts';
import { ScrollView } from 'react-native';

/**
 * The staged home's usage tab: the agent account quota of a desktop.
 *
 * Without a desktop there is no quota to show, so the tab carries the
 * original's empty state.
 */
export function OrcaUsageTab() {
  const { HomeTabHeader, HomeEmptyState } = useAgentComponents();
  return (
    <>
      <HomeTabHeader title="Usage" />
      <ScrollView>
        <HomeEmptyState
          icon="usage"
          title="No usage yet"
          message="Pair a desktop to see agent account usage."
          titleSize={17}
          bodySize={15}
        />
      </ScrollView>
    </>
  );
}
