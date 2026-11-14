import { useCallback, useMemo } from 'react';
import { FlatList } from 'react-native';
import { router } from 'expo-router';
import { useDependencyOverrides } from '@lib/app-di';
import type { OrchestrationThreadSummary } from '@app/remote-chat-agent-contracts/orchestration';
import type { ModelSelection } from '../domain';
import { Logger } from '@lib/logging';
import {
  ListItem,
  ScreenHeader,
  Text,
  View,
} from '@lib/tamagui-design-system';
import { Plus } from '@lib/tamagui-design-system/icons';

import type { OrchestrationGateway } from '../data/orchestrationGateway';
import { useGateway } from '../state/useGateway';
import { useShellSubscription } from '../state/useShellSubscription';
import { useGatewayStore } from '../state/gatewayStore';
import type { CodingAgentTransport } from '../state/gatewayStore';
import { useThreadShellStore } from '../state/threadShellStore';
import { ConnectionBanner } from './ConnectionBanner';
import { DispatchRunsSection } from './DispatchRunsSection';

const DEFAULT_MODEL_SELECTION: ModelSelection = { cli: 'opencode' };

type ThreadStatus = 'active' | 'error' | 'idle';

const STATUS_DOT_COLOR = {
  active: '$green9',
  error: '$red9',
  idle: '$gray8',
} as const;

function threadStatus(thread: OrchestrationThreadSummary): ThreadStatus {
  if (
    thread.latestTurn?.state === 'running' ||
    thread.session?.status === 'starting' ||
    thread.session?.status === 'running'
  ) {
    return 'active';
  }
  if (
    thread.latestTurn?.state === 'error' ||
    thread.session?.status === 'error'
  ) {
    return 'error';
  }
  return 'idle';
}

function formatRelativeTime(iso: string): string {
  const elapsedMs = Date.now() - Date.parse(iso);
  if (Number.isNaN(elapsedMs)) return '';
  const seconds = Math.floor(elapsedMs / 1000);
  if (seconds < 60) return 'just now';
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

function ThreadRow({
  thread,
}: {
  thread: OrchestrationThreadSummary;
}): React.ReactElement {
  const status = threadStatus(thread);
  return (
    <ListItem
      testID={`ThreadRow-${thread.id}`}
      showChevron
      onPress={() => {
        router.push(`/coding-agent/${thread.id}`);
      }}
    >
      <Text variant="strong" numberOfLines={1}>
        {thread.title}
      </Text>
      <View flexDirection="row" items="center" gap="$1.5">
        <View
          width={8}
          height={8}
          rounded={9999}
          bg={STATUS_DOT_COLOR[status]}
          testID={`ThreadRowStatus-${thread.id}`}
        />
        <Text variant="muted" size="$3">
          {formatRelativeTime(thread.updatedAt)}
        </Text>
      </View>
    </ListItem>
  );
}

export function ThreadListScreen(): React.ReactElement {
  useGateway();
  useShellSubscription();
  const threads = useThreadShellStore((state) => state.threads);
  const { gateway: overriddenGateway } =
    useDependencyOverrides<{ gateway: OrchestrationGateway }>('coding-agent');
  const dispatchTransport: CodingAgentTransport =
    overriddenGateway !== undefined ? 'fixture' : 'ws';

  const visibleThreads = useMemo(
    () => threads.filter((thread) => !thread.deletedAt),
    [threads],
  );

  const handleNewThread = useCallback(async () => {
    const gateway = useGatewayStore.getState().gateway;
    if (gateway === null) {
      Logger.info('coding-agent.list.new_thread_skipped', {
        reason: 'gateway-unavailable',
      });
      return;
    }
    const { receipt, threadId } = await useThreadShellStore
      .getState()
      .createThread(gateway, {
        title: 'New thread',
        cwd: '/',
        modelSelection: DEFAULT_MODEL_SELECTION,
        runtimeMode: 'approval-required',
        interactionMode: 'default',
      });
    if (receipt === null) {
      Logger.info('coding-agent.list.new_thread_failed', { threadId });
      return;
    }
    router.push(`/coding-agent/${threadId}`);
  }, []);

  return (
    <View flex={1} testID="ThreadListScreen">
      <ScreenHeader
        title="Coding agent"
        actions={[
          {
            icon: Plus,
            accessibilityLabel: 'New thread',
            onPress: () => {
              void handleNewThread();
            },
            testID: 'NewThreadButton',
          },
        ]}
        testID="ThreadListHeader"
      />
      <ConnectionBanner />
      {visibleThreads.length === 0 ? (
        <View
          flex={1}
          items="center"
          justify="center"
          gap="$2"
          p="$6"
          testID="ThreadListEmpty"
        >
          <Text variant="strong">No threads yet</Text>
          <Text variant="muted" size="$3">
            Start a new conversation to begin.
          </Text>
        </View>
      ) : (
        <FlatList
          testID="ThreadList"
          data={visibleThreads}
          keyExtractor={(item) => item.id}
          renderItem={({ item }) => <ThreadRow thread={item} />}
          ListFooterComponent={<DispatchRunsSection transport={dispatchTransport} />}
          contentContainerStyle={{ padding: 16, gap: 8 }}
        />
      )}
    </View>
  );
}