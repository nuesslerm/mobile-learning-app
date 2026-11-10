// features/coding-agent/src/domain/threadFeed.ts
//
// Pure derivation of the flattened chat/work timeline for a single thread.
// Ported from references/t3code/apps/mobile/src/lib/threadActivity.ts over our
// `OrchestrationThread` shapes: no `effect`, no IO, total comparators on
// (createdAt, sequence, id). Work-log rows are derived in `./workLog`.

import type {
  OrchestrationMessage,
  OrchestrationProposedPlan,
  OrchestrationThread,
  OrchestrationThreadActivity,
} from '../contracts/orchestration';

import {
  deriveThreadFeedPresentation,
  deriveWorkLogEntries,
  type WorkLogEntry,
} from './workLog';

export type ThreadFeedEntry =
  | {
      kind: 'user-message';
      id: string;
      turnId?: string;
      message: OrchestrationMessage;
    }
  | {
      kind: 'assistant-message';
      id: string;
      turnId?: string;
      message: OrchestrationMessage;
    }
  | {
      kind: 'work-log';
      id: string;
      turnId?: string;
      entry: WorkLogEntry;
    }
  | {
      kind: 'work-toggle';
      id: string;
      turnId?: string;
      groupId: string;
      hiddenCount: number;
    }
  | {
      kind: 'approval';
      id: string;
      turnId?: string;
      activity: OrchestrationThreadActivity;
    }
  | {
      kind: 'user-input';
      id: string;
      turnId?: string;
      activity: OrchestrationThreadActivity;
    }
  | {
      kind: 'proposed-plan';
      id: string;
      turnId?: string;
      plan: OrchestrationProposedPlan;
    }
  | {
      kind: 'error';
      id: string;
      turnId?: string;
      activity: OrchestrationThreadActivity;
    }
  | {
      kind: 'working';
      id: string;
      turnId?: string;
    };

interface RawFeedEntry {
  id: string;
  createdAt: string;
  sequence?: number;
  turnId?: string;
  entry: ThreadFeedEntry;
}

function compareRawEntries(left: RawFeedEntry, right: RawFeedEntry): number {
  return (
    left.createdAt.localeCompare(right.createdAt) ||
    (left.sequence ?? 0) - (right.sequence ?? 0) ||
    left.id.localeCompare(right.id)
  );
}

/**
 * Group consecutive activities that share a turnId into runs. Adjacent
 * activities across a message boundary are intentionally not merged; callers
 * split on message rows before grouping.
 */
export function groupAdjacentActivities(
  activities: ReadonlyArray<OrchestrationThreadActivity>
): OrchestrationThreadActivity[][] {
  const groups: OrchestrationThreadActivity[][] = [];
  let openGroup: OrchestrationThreadActivity[] | null = null;
  let openTurnId: string | undefined;

  for (const activity of activities) {
    if (openGroup !== null && openTurnId === activity.turnId) {
      openGroup.push(activity);
      continue;
    }
    openGroup = [activity];
    openTurnId = activity.turnId;
    groups.push(openGroup);
  }

  return groups;
}

/**
 * Build the flattened timeline for a thread: user/assistant messages,
 * proposed plans, and activity groups interleaved by (createdAt, sequence, id)
 * with a total comparator. A trailing `working` entry is emitted while the
 * latest turn is still running. Within an activity group, work-log rows are
 * capped at `MAX_VISIBLE_WORK_LOG_ENTRIES` and the overflow is folded behind a
 * `work-toggle` row carrying the hidden count.
 */
export function buildThreadFeed(
  thread: OrchestrationThread,
  expandedGroupIds: ReadonlySet<string> = new Set()
): ThreadFeedEntry[] {
  const raw: RawFeedEntry[] = [];

  for (const message of thread.messages) {
    raw.push({
      id: message.id,
      createdAt: message.createdAt,
      turnId: message.turnId,
      entry: {
        kind: message.role === 'user' ? 'user-message' : 'assistant-message',
        id: message.id,
        turnId: message.turnId,
        message,
      },
    });
  }

  for (const plan of thread.proposedPlans) {
    raw.push({
      id: plan.id,
      createdAt: plan.createdAt,
      turnId: plan.turnId,
      entry: { kind: 'proposed-plan', id: plan.id, turnId: plan.turnId, plan },
    });
  }

  for (const group of groupAdjacentActivities(thread.activities)) {
    const groupTurnId = group[0]?.turnId;
    const workLogs = deriveWorkLogEntries(group);
    const workLogIds = new Set(workLogs.map((entry) => entry.id));
    const nonTool = group.filter(
      (activity) =>
        activity.tone !== 'tool' &&
        activity.kind !== 'turn.diff.updated' &&
        !workLogIds.has(activity.id)
    );
    for (const activity of nonTool) {
      const kind =
        activity.tone === 'approval'
          ? 'approval'
          : activity.tone === 'error'
            ? 'error'
            : 'user-input';
      raw.push({
        id: activity.id,
        createdAt: activity.createdAt,
        sequence: activity.sequence,
        turnId: activity.turnId,
        entry: { kind, id: activity.id, turnId: activity.turnId, activity },
      });
    }

    const presented = deriveThreadFeedPresentation(workLogs, expandedGroupIds);
    for (const entry of presented) {
      raw.push({
        id: entry.id,
        createdAt: group[0]?.createdAt ?? '',
        sequence: group[0]?.sequence,
        turnId: groupTurnId,
        entry: { ...entry, turnId: groupTurnId },
      });
    }
  }

  raw.sort(compareRawEntries);

  const feed = raw.map((item) => item.entry);

  if (thread.latestTurn?.state === 'running') {
    feed.push({
      kind: 'working',
      id: 'working-indicator-row',
      turnId: thread.latestTurn.turnId,
    });
  }

  return feed;
}
