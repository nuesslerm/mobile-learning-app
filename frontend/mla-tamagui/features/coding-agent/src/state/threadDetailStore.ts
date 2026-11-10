// features/coding-agent/src/state/threadDetailStore.ts
//
// Zustand store that owns the per-thread detail view: a `Record<ThreadId,
// ThreadDetailState>` (the snapshot-reducer keeps threads loaded on demand
// via `subscribeThread`) plus a global `expandedGroupIds` set the
// work-toggle rows in the feed flip. `applyItem(threadId, item)` is the
// single mutation point: `synchronized` is a no-op, `snapshot` replaces
// the entry (with a fresh `turnDiffs: {}` so the per-turn diff map never
// survives a snapshot boundary), and `event` runs the pure
// `applyThreadDetailEvent` reducer from `src/domain`. The
// selectors (`feed`, `pendingApprovals`, `pendingUserInputs`,
// `presentation`) read the cached thread and run the matching pure
// derivations from `src/domain`, so the work-log
// collapse and the approval/user-input derivation stay in the domain
// package and are unit tested without React.
//
// `presentation` rebuilds a feed entry list against the current
// `expandedGroupIds` set — the caller passes the per-thread work-log
// entries and gets the visible rows plus any toggle entries, so the
// toggle rows stay owned by the feed component (they need to render next
// to the rows they hide, not in a separate selector surface).
import { Logger } from '@lib/logging';
import {
  applyThreadDetailEvent,
  buildThreadFeed,
  derivePendingApprovals,
  derivePendingUserInputs,
  deriveThreadFeedPresentation,
  type PendingApproval,
  type PendingUserInput,
  type ThreadDetailState,
  type ThreadFeedEntry,
  type WorkLogEntry,
} from '../domain';
import type {
  OrchestrationSubscribeThreadItem,
  ThreadId,
} from '@app/remote-chat-agent-contracts/orchestration';
import { create } from 'zustand';

type ThreadDetailStoreState = {
  threads: Record<ThreadId, ThreadDetailState>;
  /** Threads whose snapshot-less events were already warned about: one warning, not one per streamed event. */
  noSnapshotWarned: Record<ThreadId, true>;
  expandedGroupIds: Set<string>;
  applyItem: (
    threadId: ThreadId,
    item: OrchestrationSubscribeThreadItem,
  ) => void;
  getThread: (threadId: ThreadId) => ThreadDetailState | undefined;
  removeThread: (threadId: ThreadId) => void;
  toggleExpandedGroup: (groupId: string) => void;
  feed: (threadId: ThreadId) => ThreadFeedEntry[];
  pendingApprovals: (threadId: ThreadId) => PendingApproval[];
  pendingUserInputs: (threadId: ThreadId) => PendingUserInput[];
  presentation: (entries: readonly WorkLogEntry[]) => ThreadFeedEntry[];
};

const LAST_ERROR_MAX = 200;

/** Log fields for where a run stands; `lastError` is the provider's message, cut short, never message text. */
export function threadSessionFields(
  thread: Pick<ThreadDetailState, 'latestTurn' | 'session'> | undefined,
): {
  turnState: string | undefined;
  sessionStatus: string | undefined;
  lastError: string | undefined;
} {
  return {
    turnState: thread?.latestTurn?.state,
    sessionStatus: thread?.session?.status,
    lastError: thread?.session?.lastError?.slice(0, LAST_ERROR_MAX),
  };
}

function logTurnState(
  threadId: ThreadId,
  previous: ThreadDetailState | undefined,
  next: ThreadDetailState,
): void {
  const from = threadSessionFields(previous);
  const to = threadSessionFields(next);
  if (
    previous === undefined ||
    (from.turnState === to.turnState && from.sessionStatus === to.sessionStatus)
  ) {
    return;
  }
  const fields = {
    threadId,
    turnId: next.latestTurn?.turnId,
    from: from.turnState,
    to: to.turnState,
    sessionStatus: to.sessionStatus,
    lastError: to.lastError,
  };
  if (to.turnState === 'error' || to.sessionStatus === 'error') {
    Logger.warn('coding-agent.turn.state', fields);
  } else {
    Logger.info('coding-agent.turn.state', fields);
  }
}

export const useThreadDetailStore = create<ThreadDetailStoreState>((set, get) => ({
  threads: {},
  noSnapshotWarned: {},
  expandedGroupIds: new Set<string>(),
  applyItem: (threadId, item) => {
    Logger.debug('coding-agent.store.thread_detail.apply_item', {
      threadId,
      kind: item.kind,
      ...(item.kind === 'event' ? { type: item.event.type } : {}),
    });
    if (item.kind === 'synchronized') return;
    const { threads, noSnapshotWarned } = get();
    const previous = threads[threadId];
    let next: ThreadDetailState;
    if (item.kind === 'snapshot') {
      next = { ...item.snapshot, turnDiffs: {} };
    } else {
      if (previous === undefined) {
        if (noSnapshotWarned[threadId] !== true) {
          set({ noSnapshotWarned: { ...noSnapshotWarned, [threadId]: true } });
          Logger.warn('coding-agent.thread.event_dropped', {
            threadId,
            type: item.event.type,
            sequence: item.event.sequence,
            reason: 'no-snapshot',
          });
        }
        return;
      }
      next = applyThreadDetailEvent(previous, item.event);
    }
    if (next === previous) return;
    set((state) => {
      const { [threadId]: _warned, ...stillWarned } = state.noSnapshotWarned;
      return {
        threads: { ...state.threads, [threadId]: next },
        noSnapshotWarned: stillWarned,
      };
    });
    logTurnState(threadId, previous, next);
  },
  getThread: (threadId) => get().threads[threadId],
  removeThread: (threadId) => {
    set((state) => {
      if (state.threads[threadId] === undefined) return state;
      const next = { ...state.threads };
      delete next[threadId];
      return { threads: next };
    });
  },
  toggleExpandedGroup: (groupId) => {
    set((state) => {
      const next = new Set(state.expandedGroupIds);
      const expanded = !next.has(groupId);
      if (expanded) {
        next.add(groupId);
      } else {
        next.delete(groupId);
      }
      Logger.debug('coding-agent.store.thread_detail.toggle_expanded_group', {
        groupId,
        expanded,
      });
      return { expandedGroupIds: next };
    });
  },
  feed: (threadId) => {
    const thread = get().threads[threadId];
    if (thread === undefined) return [];
    return buildThreadFeed(thread, get().expandedGroupIds);
  },
  pendingApprovals: (threadId) => {
    const thread = get().threads[threadId];
    if (thread === undefined) return [];
    return derivePendingApprovals(thread);
  },
  pendingUserInputs: (threadId) => {
    const thread = get().threads[threadId];
    if (thread === undefined) return [];
    return derivePendingUserInputs(thread);
  },
  presentation: (entries) => {
    return deriveThreadFeedPresentation(entries, get().expandedGroupIds);
  },
}));