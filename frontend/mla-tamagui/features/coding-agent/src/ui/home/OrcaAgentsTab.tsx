import { useAgentComponents } from '@lib/agent-ui-contracts';
import { useState } from 'react';
import { ScrollView } from 'react-native';

/**
 * The staged home's agents tab: every agent on every paired desktop.
 *
 * Without a desktop there is nothing to list, so the tab carries the original's
 * empty state and the tools that shape the list once there is one: the search
 * field and the options sheet.
 */
export function OrcaAgentsTab() {
  const { HomeTabHeader, HomeEmptyState, HomeSearchField, HomeActionSheet } =
    useAgentComponents();
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [menu, setMenu] = useState<'closed' | 'options' | 'group-by'>('closed');
  const [unreadOnly, setUnreadOnly] = useState(false);
  const [showWorkers, setShowWorkers] = useState(false);
  const [groupBy, setGroupBy] = useState('Status');

  const trimmed = query.trim();
  const options = [
    [
      {
        id: 'group-by',
        label: `Group by: ${groupBy}`,
        onPress: () => setMenu('group-by'),
      },
      {
        id: 'unread-only',
        label: 'Unread only',
        checked: unreadOnly,
        onPress: () => setUnreadOnly((value) => !value),
      },
      {
        id: 'show-workers',
        label: 'Show worker agents',
        checked: showWorkers,
        onPress: () => setShowWorkers((value) => !value),
      },
    ],
    [
      // No agent has run yet, so there is nothing to mark read or to clear.
      { id: 'mark-all-read', label: 'Mark all read', disabled: true, onPress: () => undefined },
      { id: 'clear-finished', label: 'Clear finished', disabled: true, onPress: () => undefined },
    ],
  ];
  const groups =
    menu === 'group-by'
      ? [
          ['Status', 'Workspace', 'Project', 'Agent', 'Desktop'].map((option) => ({
            id: option,
            label: option,
            checked: option === groupBy,
            onPress: () => {
              setGroupBy(option);
              setMenu('options');
            },
          })),
        ]
      : options;

  return (
    <>
      <HomeTabHeader
        title="Agents"
        actions={[
          {
            id: 'search',
            accessibilityLabel: 'Search agents',
            icon: 'search',
            active: searchOpen,
            onPress: () => {
              setSearchOpen((open) => !open);
              setQuery('');
            },
          },
          {
            id: 'options',
            accessibilityLabel: 'Agent list options',
            icon: 'options',
            active: menu !== 'closed',
            onPress: () => setMenu('options'),
          },
        ]}
      />
      {searchOpen ? (
        <HomeSearchField
          value={query}
          onChangeText={setQuery}
          placeholder="Search agents"
          onClear={() => setQuery('')}
        />
      ) : null}
      <ScrollView keyboardShouldPersistTaps="handled">
        {trimmed ? (
          <HomeEmptyState
            icon="agents"
            title={`No agents match “${trimmed}”`}
            iconSize={30}
            iconStrokeWidth={1.6}
            titleSize={16}
            bodySize={14}
            titleGap={8}
          />
        ) : (
          <HomeEmptyState
            icon="agents"
            title="No agents running"
            message="Pair a desktop to see its agents here."
            iconSize={30}
            iconStrokeWidth={1.6}
            titleSize={16}
            bodySize={14}
            titleGap={8}
          />
        )}
      </ScrollView>
      <HomeActionSheet
        visible={menu !== 'closed'}
        groups={groups}
        onClose={() => setMenu('closed')}
      />
    </>
  );
}
