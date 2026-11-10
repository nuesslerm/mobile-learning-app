import { useCallback, useEffect, useState } from 'react';
import { router } from 'expo-router';
import type { ThreadId } from '@app/remote-chat-agent-contracts/orchestration';
import { Logger } from '@lib/logging';
import { ScreenHeader, Text, View } from '@lib/tamagui-design-system';
import { Settings } from '@lib/tamagui-design-system/icons';

import { useGateway } from '../integration/useGateway';
import { useThreadCommands } from '../integration/useThreadCommands';
import { useThreadFeed } from '../integration/useThreadFeed';
import { useThreadSubscription } from '../integration/useThreadSubscription';
import { useThreadSettingsRequestStore } from '../state/threadSettingsRequestStore';
import { ConnectionBanner } from './ConnectionBanner';
import { PendingApprovalCard } from './PendingApprovalCard';
import { PendingUserInputCard } from './PendingUserInputCard';
import { ThreadComposer } from './ThreadComposer';
import { ThreadFeed } from './feed/ThreadFeed';
import { ThreadSettingsSheet } from './ThreadSettingsSheet';

export type ThreadScreenProps = {
  threadId?: string;
};

export function ThreadScreen({
  threadId,
}: ThreadScreenProps): React.ReactElement {
  const id = (threadId ?? '') as ThreadId;
  // The export prerenders this route without the id, so the header shows it only after mount.
  const [shownId, setShownId] = useState<string>();
  useEffect(() => setShownId(threadId), [threadId]);
  useGateway();
  useThreadSubscription(id);
  const { feed, pendingApprovals, pendingUserInputs, liveFollow, setLiveFollowEvent } =
    useThreadFeed(id);
  const { respondApproval, respondUserInput, implementPlan } = useThreadCommands(id);

  const openSettings = useCallback(() => {
    if (threadId === undefined) return;
    Logger.info('coding-agent.thread.settings_open', { threadId });
    useThreadSettingsRequestStore.getState().request({ threadId });
  }, [threadId]);

  return (
    <View flex={1} testID="ThreadScreen">
      <ScreenHeader
        title="Coding agent"
        subtitle={shownId}
        onBack={() => router.back()}
        actions={[
          {
            icon: Settings,
            accessibilityLabel: 'Thread settings',
            onPress: openSettings,
            testID: 'ThreadSettingsButton',
          },
        ]}
        testID="ThreadScreenHeader"
      />
      <ConnectionBanner />
      {threadId === undefined ? (
        <View
          flex={1}
          items="center"
          justify="center"
          p="$6"
          testID="ThreadScreenMissingId"
        >
          <Text variant="muted">No thread selected.</Text>
        </View>
      ) : (
        <>
          <ThreadFeed
            feed={feed}
            liveFollow={liveFollow}
            setLiveFollowEvent={setLiveFollowEvent}
            onImplementPlan={(plan) => {
              void implementPlan(plan);
            }}
          />
          {pendingApprovals.map((approval) => (
            <PendingApprovalCard
              key={approval.requestId}
              approval={approval}
              onRespond={(requestId, decision) =>
                void respondApproval(requestId, decision)
              }
            />
          ))}
          {pendingUserInputs.map((pending) => (
            <PendingUserInputCard
              key={pending.requestId}
              pending={pending}
              onRespond={(requestId, answers) =>
                void respondUserInput(requestId, answers)
              }
            />
          ))}
          <ThreadComposer threadId={id} />
          <ThreadSettingsSheet />
        </>
      )}
    </View>
  );
}