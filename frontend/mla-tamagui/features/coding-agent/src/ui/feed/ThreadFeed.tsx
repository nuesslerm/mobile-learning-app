import { useCallback, useMemo, useRef } from 'react';
import {
  FlatList,
  type ListRenderItem,
  type NativeScrollEvent,
  type NativeSyntheticEvent,
} from 'react-native';
import type { ThreadFeedEntry, ThreadFeedLiveFollowEvent } from '../../domain';
import type { OrchestrationProposedPlan } from '@app/remote-chat-agent-contracts/orchestration';

import { AssistantMessageItem } from './AssistantMessageItem';
import { ProposedPlanCard } from './ProposedPlanCard';
import { TurnFold } from './TurnFold';
import { UserMessageItem } from './UserMessageItem';
import { WorkGroupToggle } from './WorkGroupToggle';
import { WorkLogGroup } from './WorkLogGroup';
import { WorkingIndicator } from './WorkingIndicator';

const FOLLOW_END_THRESHOLD = 8;

function computeWorkLogGroupIds(
  feed: readonly ThreadFeedEntry[]
): ReadonlyMap<string, string> {
  const groupByEntryId = new Map<string, string>();
  let openGroupId: string | undefined;
  for (let index = feed.length - 1; index >= 0; index--) {
    const item = feed[index];
    if (item.kind === 'work-toggle') {
      openGroupId = item.groupId;
    } else if (item.kind === 'work-log') {
      groupByEntryId.set(item.id, openGroupId ?? item.id);
    } else {
      openGroupId = undefined;
    }
  }
  return groupByEntryId;
}

export type ThreadFeedProps = {
  feed: ThreadFeedEntry[];
  liveFollow: boolean;
  setLiveFollowEvent: (event: ThreadFeedLiveFollowEvent) => void;
  /** Reasoning text per turnId, shown collapsed under the Brain chip on that turn's assistant message. */
  reasoningByTurnId?: ReadonlyMap<string, string>;
  /** turnId of the running turn -- its assistant message shows the streaming caret. */
  streamingTurnId?: string;
  /** Called when the user implements a proposed plan; wires to startTurn with sourceProposedPlan. */
  onImplementPlan?: (plan: OrchestrationProposedPlan) => void;
};

function computeIsAtEnd(
  event: NativeSyntheticEvent<NativeScrollEvent>
): boolean {
  const { contentOffset, contentSize, layoutMeasurement } = event.nativeEvent;
  return (
    contentOffset.y + layoutMeasurement.height >=
    contentSize.height - FOLLOW_END_THRESHOLD
  );
}

export function ThreadFeed({
  feed,
  liveFollow,
  setLiveFollowEvent,
  reasoningByTurnId,
  streamingTurnId,
  onImplementPlan,
}: ThreadFeedProps): React.ReactElement {
  const listRef = useRef<FlatList<ThreadFeedEntry>>(null);
  const userScrollSessionRef = useRef(false);
  const workLogGroupIds = useMemo(() => computeWorkLogGroupIds(feed), [feed]);

  const handleScrollBeginDrag = useCallback(() => {
    userScrollSessionRef.current = true;
    setLiveFollowEvent({ type: 'user-scroll-begin' });
  }, [setLiveFollowEvent]);

  const handleScroll = useCallback(
    (event: NativeSyntheticEvent<NativeScrollEvent>) => {
      setLiveFollowEvent({
        type: 'scroll',
        isAtEnd: computeIsAtEnd(event),
        userScrollSessionActive: userScrollSessionRef.current,
      });
    },
    [setLiveFollowEvent]
  );

  const handleScrollEndDrag = useCallback(
    (event: NativeSyntheticEvent<NativeScrollEvent>) => {
      userScrollSessionRef.current = false;
      setLiveFollowEvent({
        type: 'user-scroll-end',
        isAtEnd: computeIsAtEnd(event),
        userScrollSessionActive: true,
      });
    },
    [setLiveFollowEvent]
  );

  const renderItem = useCallback<ListRenderItem<ThreadFeedEntry>>(
    ({ item }) => {
      switch (item.kind) {
        case 'user-message':
          return <UserMessageItem entry={item} />;
        case 'assistant-message':
          return (
            <AssistantMessageItem
              entry={item}
              reasoning={reasoningByTurnId?.get(item.turnId ?? '')}
              streaming={
                streamingTurnId !== undefined && item.turnId === streamingTurnId
              }
            />
          );
        case 'work-log':
          return (
            <WorkLogGroup
              groupId={workLogGroupIds.get(item.id) ?? item.id}
              entries={[item.entry]}
            />
          );
        case 'work-toggle':
          return (
            <WorkGroupToggle
              groupId={item.groupId}
              hiddenCount={item.hiddenCount}
            />
          );
        case 'approval':
        case 'user-input':
        case 'error':
          return null;
        case 'proposed-plan':
          return (
            <TurnFold
              turnId={item.turnId ?? 'unknown'}
              title="Turn · completed"
            >
              <ProposedPlanCard entry={item} onImplement={onImplementPlan} />
            </TurnFold>
          );
        case 'working':
          return <WorkingIndicator />;
      }
    },
    [reasoningByTurnId, streamingTurnId, workLogGroupIds, onImplementPlan]
  );

  return (
    <FlatList
      ref={listRef}
      testID="ThreadFeed"
      style={{ flex: 1 }}
      data={feed}
      keyExtractor={(item) => item.id}
      renderItem={renderItem}
      contentContainerStyle={{ padding: 16, paddingBottom: 24 }}
      scrollEventThrottle={16}
      keyboardShouldPersistTaps="handled"
      onScrollBeginDrag={handleScrollBeginDrag}
      onScroll={handleScroll}
      onScrollEndDrag={handleScrollEndDrag}
      onContentSizeChange={() => {
        if (liveFollow) {
          listRef.current?.scrollToEnd({ animated: false });
        }
      }}
    />
  );
}