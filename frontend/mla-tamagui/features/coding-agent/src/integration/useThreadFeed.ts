import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  buildThreadFeed,
  derivePendingApprovals,
  derivePendingUserInputs,
  resolveThreadFeedLiveFollow,
  type PendingApproval,
  type PendingUserInput,
  type ThreadFeedEntry,
  type ThreadFeedLiveFollowEvent,
} from '../domain';
import type { ThreadId } from '@app/remote-chat-agent-contracts/orchestration';

import { useThreadDetailStore } from '../state/threadDetailStore';

export type ThreadFeedState = {
  feed: ThreadFeedEntry[];
  pendingApprovals: PendingApproval[];
  pendingUserInputs: PendingUserInput[];
  liveFollow: boolean;
  setLiveFollowEvent: (event: ThreadFeedLiveFollowEvent) => void;
};

export function useThreadFeed(threadId: ThreadId): ThreadFeedState {
  const thread = useThreadDetailStore((state) => state.threads[threadId]);
  const expandedGroupIds = useThreadDetailStore((state) => state.expandedGroupIds);
  const [liveFollow, setLiveFollow] = useState(true);
  const previousThreadId = useRef(threadId);

  useEffect(() => {
    if (previousThreadId.current === threadId) return;
    previousThreadId.current = threadId;
    setLiveFollow(true);
  }, [threadId]);

  const feed = useMemo(
    () => (thread === undefined ? [] : buildThreadFeed(thread, expandedGroupIds)),
    [thread, expandedGroupIds],
  );
  const pendingApprovals = useMemo(
    () => (thread === undefined ? [] : derivePendingApprovals(thread)),
    [thread],
  );
  const pendingUserInputs = useMemo(
    () => (thread === undefined ? [] : derivePendingUserInputs(thread)),
    [thread],
  );
  const setLiveFollowEvent = useCallback((event: ThreadFeedLiveFollowEvent) => {
    setLiveFollow((current) => resolveThreadFeedLiveFollow(current, event));
  }, []);

  const result = useMemo(
    () => ({
      feed,
      pendingApprovals,
      pendingUserInputs,
      liveFollow,
      setLiveFollowEvent,
    }),
    [feed, liveFollow, pendingApprovals, pendingUserInputs, setLiveFollowEvent],
  );

  return result;
}
