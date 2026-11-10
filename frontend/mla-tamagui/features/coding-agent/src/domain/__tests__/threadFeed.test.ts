// features/coding-agent/src/domain/__tests__/threadFeed.test.ts
//
// Ported in structure from
// references/t3code/apps/mobile/src/lib/threadActivity.test.ts onto our
// `OrchestrationThread` shapes and simplified `ThreadFeedEntry` union. Every
// test is Given/When/Then; threads are built with the `makeThread` / `makeActivity`
// helpers (branded ids via `.parse`).

import {
  EventId,
  MessageId,
  ThreadId,
  TurnId,
  type OrchestrationThread,
  type OrchestrationThreadActivity,
} from '@app/remote-chat-agent-contracts/orchestration';
import { describe, expect, it } from 'vitest';

import {
  buildThreadFeed,
  groupAdjacentActivities,
  type ThreadFeedEntry,
} from '../threadFeed';
import { deriveWorkLogEntries, MAX_VISIBLE_WORK_LOG_ENTRIES } from '../workLog';

function makeThread(
  overrides: Partial<OrchestrationThread> = {}
): OrchestrationThread {
  return {
    id: ThreadId.parse('thread-1'),
    title: 'Test Thread',
    cwd: '/repo',
    modelSelection: { cli: 'codex', modelID: 'gpt-5.4' },
    runtimeMode: 'full-access',
    interactionMode: 'default',
    latestTurn: null,
    createdAt: '2026-04-01T00:00:00.000Z',
    updatedAt: '2026-04-01T00:00:00.000Z',
    archivedAt: null,
    deletedAt: null,
    messages: [],
    proposedPlans: [],
    activities: [],
    session: null,
    ...overrides,
  };
}

function makeActivity(
  overrides: Partial<OrchestrationThreadActivity>
): OrchestrationThreadActivity {
  return {
    id: EventId.parse('activity-1'),
    tone: 'tool',
    kind: 'tool.completed',
    summary: 'Run tests',
    payload: null,
    createdAt: '2026-04-01T00:00:01.000Z',
    ...overrides,
  };
}

function makeMessage(
  overrides: Partial<OrchestrationThread['messages'][number]>
): OrchestrationThread['messages'][number] {
  return {
    id: MessageId.parse('message-1'),
    role: 'user',
    text: 'Hello',
    createdAt: '2026-04-01T00:00:00.000Z',
    updatedAt: '2026-04-01T00:00:00.000Z',
    ...overrides,
  };
}

describe('groupAdjacentActivities', () => {
  it('given activities across two turns, when grouped, then adjacent same-turn runs are merged', () => {
    // Given a stream of activities alternating turns
    const activities = [
      makeActivity({
        id: EventId.parse('activity-old'),
        kind: 'tool.completed',
        summary: 'Old work',
        createdAt: '2026-04-01T00:00:02.000Z',
        turnId: TurnId.parse('turn-old'),
      }),
      makeActivity({
        id: EventId.parse('activity-latest'),
        kind: 'tool.completed',
        summary: 'Latest work',
        createdAt: '2026-04-01T00:00:03.000Z',
        turnId: TurnId.parse('turn-latest'),
      }),
    ];
    // When grouped
    const groups = groupAdjacentActivities(activities);
    // Then each turn becomes its own run
    expect(groups.map((group) => group.map((a) => a.id))).toEqual([
      ['activity-old'],
      ['activity-latest'],
    ]);
  });

  it('given consecutive same-turn activities, when grouped, then they collapse into one run', () => {
    // Given two tool activities in the same turn
    const turnId = TurnId.parse('turn-1');
    const activities = [
      makeActivity({
        id: EventId.parse('tool-1'),
        createdAt: '2026-04-01T00:00:01.000Z',
        turnId,
      }),
      makeActivity({
        id: EventId.parse('tool-2'),
        createdAt: '2026-04-01T00:00:02.000Z',
        turnId,
      }),
    ];
    // When grouped
    const groups = groupAdjacentActivities(activities);
    // Then they collapse into a single run
    expect(groups).toHaveLength(1);
    expect(groups[0].map((a) => a.id)).toEqual(['tool-1', 'tool-2']);
  });
});

describe('deriveWorkLogEntries', () => {
  it('given a tool activity, when derived, then a stub work-log entry with title = summary is returned', () => {
    // Given a single tool activity
    const activity = makeActivity({
      id: EventId.parse('tool-completed'),
      summary: 'Run tests',
    });
    // When derived
    const entries = deriveWorkLogEntries([activity]);
    // Then one entry is returned with title = summary and the tool kind
    expect(entries).toEqual([
      {
        id: 'tool-completed',
        kind: 'tool',
        title: 'Run tests',
        status: 'done',
      },
    ]);
  });

  it('given a non-tool activity, when derived, then no work-log entry is returned', () => {
    // Given an info-toned activity
    const activity = makeActivity({
      id: EventId.parse('info-1'),
      tone: 'info',
      kind: 'runtime.warning',
      summary: 'Runtime warning',
    });
    // When derived
    const entries = deriveWorkLogEntries([activity]);
    // Then it is not treated as work
    expect(entries).toEqual([]);
  });
});

describe('diff entries from turn.diff.updated activities', () => {
  it('given the fx-diff activities, when the feed is built, then exactly one diff entry carries the patch', () => {
    // Given a turn with a turn.diff.updated activity holding a unified diff
    const patch = [
      '--- a/src/foo.ts',
      '+++ b/src/foo.ts',
      '@@ -1,3 +1,3 @@',
      '-export const a = 1;',
      '+export const a = 2;',
      '',
    ].join('\n');
    const turnId = TurnId.parse('turn-diff');
    const thread = makeThread({
      latestTurn: {
        turnId,
        state: 'running',
        requestedAt: '2026-04-01T00:00:00.000Z',
        startedAt: '2026-04-01T00:00:00.000Z',
        completedAt: null,
      },
      activities: [
        makeActivity({
          id: EventId.parse('fx-diff-activity-1'),
          tone: 'info',
          kind: 'turn.diff.updated',
          summary: 'Diff for current turn',
          payload: { turnId: 'turn-diff', unifiedDiff: patch },
          turnId,
          createdAt: '2026-04-01T00:00:01.000Z',
        }),
      ],
    });
    // When built
    const feed = buildThreadFeed(thread);
    // Then exactly one diff work-log entry carries the patch and no stray row is emitted
    const diffEntries = feed.filter(
      (entry): entry is Extract<ThreadFeedEntry, { kind: 'work-log' }> =>
        entry.kind === 'work-log' && entry.entry.kind === 'diff'
    );
    expect(diffEntries).toHaveLength(1);
    expect(diffEntries[0].entry).toMatchObject({
      id: 'fx-diff-activity-1',
      kind: 'diff',
      title: 'Changes',
      status: 'done',
      diff: patch,
    });
    expect(diffEntries[0].turnId).toBe('turn-diff');
    expect(feed.some((entry) => entry.kind === 'user-input')).toBe(false);
  });

  it('given two turn.diff.updated activities for one turn, when built, then only the latest patch is kept', () => {
    // Given two diff updates for the same turn, the newer arriving later
    const turnId = TurnId.parse('turn-diff');
    const thread = makeThread({
      activities: [
        makeActivity({
          id: EventId.parse('diff-old'),
          tone: 'info',
          kind: 'turn.diff.updated',
          summary: 'Diff for current turn',
          payload: { turnId: 'turn-diff', unifiedDiff: 'old patch' },
          turnId,
          createdAt: '2026-04-01T00:00:01.000Z',
        }),
        makeActivity({
          id: EventId.parse('diff-latest'),
          tone: 'info',
          kind: 'turn.diff.updated',
          summary: 'Diff for current turn',
          payload: { turnId: 'turn-diff', unifiedDiff: 'new patch' },
          turnId,
          createdAt: '2026-04-01T00:00:02.000Z',
        }),
      ],
    });
    // When built
    const feed = buildThreadFeed(thread);
    // Then only the latest diff entry is kept
    const diffEntries = feed.filter(
      (entry): entry is Extract<ThreadFeedEntry, { kind: 'work-log' }> =>
        entry.kind === 'work-log' && entry.entry.kind === 'diff'
    );
    expect(diffEntries).toHaveLength(1);
    expect(diffEntries[0].entry.diff).toBe('new patch');
  });
});

describe('buildThreadFeed', () => {
  it('given a thread with messages, activities, and a plan, when built, then entries are interleaved by createdAt', () => {
    // Given a user message, a tool activity, and a proposed plan
    const thread = makeThread({
      latestTurn: {
        turnId: TurnId.parse('turn-1'),
        state: 'completed',
        requestedAt: '2026-04-01T00:00:00.000Z',
        startedAt: '2026-04-01T00:00:00.000Z',
        completedAt: '2026-04-01T00:00:05.000Z',
      },
      messages: [
        makeMessage({
          id: MessageId.parse('user-1'),
          role: 'user',
          text: 'Do it',
          createdAt: '2026-04-01T00:00:00.000Z',
        }),
      ],
      proposedPlans: [
        {
          id: 'plan-1',
          turnId: TurnId.parse('turn-1'),
          planMarkdown: '## Plan',
          createdAt: '2026-04-01T00:00:04.000Z',
          updatedAt: '2026-04-01T00:00:04.000Z',
        },
      ],
      activities: [
        makeActivity({
          id: EventId.parse('tool-1'),
          createdAt: '2026-04-01T00:00:02.000Z',
          turnId: TurnId.parse('turn-1'),
        }),
      ],
    });
    // When built
    const feed = buildThreadFeed(thread);
    // Then messages, work-log groups, and plans are interleaved in time order
    expect(feed.map((entry) => entry.kind)).toEqual([
      'user-message',
      'work-log',
      'proposed-plan',
    ]);
  });

  it('given a running latest turn, when built, then a trailing working entry is emitted', () => {
    // Given a thread whose latest turn is running with a tool activity
    const thread = makeThread({
      latestTurn: {
        turnId: TurnId.parse('turn-running'),
        state: 'running',
        requestedAt: '2026-04-01T00:00:00.000Z',
        startedAt: '2026-04-01T00:00:01.000Z',
        completedAt: null,
      },
      activities: [
        makeActivity({
          id: EventId.parse('tool-1'),
          createdAt: '2026-04-01T00:00:05.000Z',
          turnId: TurnId.parse('turn-running'),
        }),
      ],
    });
    // When built
    const feed = buildThreadFeed(thread);
    // Then the last entry is the working indicator for the running turn
    expect(feed.at(-1)).toMatchObject({
      kind: 'working',
      id: 'working-indicator-row',
      turnId: 'turn-running',
    });
  });

  it('given a settled latest turn, when built, then no trailing working entry is emitted', () => {
    // Given a thread whose latest turn is completed
    const thread = makeThread({
      latestTurn: {
        turnId: TurnId.parse('turn-1'),
        state: 'completed',
        requestedAt: '2026-04-01T00:00:00.000Z',
        startedAt: '2026-04-01T00:00:01.000Z',
        completedAt: '2026-04-01T00:00:03.000Z',
      },
      activities: [
        makeActivity({
          id: EventId.parse('tool-1'),
          createdAt: '2026-04-01T00:00:02.000Z',
          turnId: TurnId.parse('turn-1'),
        }),
      ],
    });
    // When built
    const feed = buildThreadFeed(thread);
    // Then there is no working entry
    expect(feed.some((entry) => entry.kind === 'working')).toBe(false);
  });

  it('given multiple work-log activities in one turn, when built, then overflow folds behind a work-toggle', () => {
    // Given a thread with more work-log entries than MAX_VISIBLE_WORK_LOG_ENTRIES in one turn
    const turnId = TurnId.parse('turn-1');
    const thread = makeThread({
      latestTurn: {
        turnId,
        state: 'completed',
        requestedAt: '2026-04-01T00:00:00.000Z',
        startedAt: '2026-04-01T00:00:00.000Z',
        completedAt: '2026-04-01T00:00:05.000Z',
      },
      activities: [1, 2, 3].map((index) =>
        makeActivity({
          id: EventId.parse(`tool-${index}`),
          createdAt: `2026-04-01T00:00:0${index}.000Z`,
          turnId,
        })
      ),
    });
    // When built
    const feed = buildThreadFeed(thread);
    // Then only the newest work-log stays visible plus a work-toggle with hiddenCount
    expect(MAX_VISIBLE_WORK_LOG_ENTRIES).toBe(1);
    expect(feed.filter((entry) => entry.kind === 'work-log')).toHaveLength(1);
    const toggle = feed.find((entry) => entry.kind === 'work-toggle') as
      | Extract<ThreadFeedEntry, { kind: 'work-toggle' }>
      | undefined;
    expect(toggle).toMatchObject({
      kind: 'work-toggle',
      hiddenCount: 2,
    });
  });

  it('given an approval-toned activity, when built, then an approval entry is emitted', () => {
    // Given an approval activity
    const thread = makeThread({
      activities: [
        makeActivity({
          id: EventId.parse('approval-1'),
          tone: 'approval',
          kind: 'approval.requested',
          summary: 'Approve command',
          createdAt: '2026-04-01T00:00:01.000Z',
        }),
      ],
    });
    // When built
    const feed = buildThreadFeed(thread);
    // Then the activity surfaces as an approval entry
    expect(feed[0]).toMatchObject({ kind: 'approval', id: 'approval-1' });
  });

  it('given an error-toned activity, when built, then an error entry is emitted', () => {
    // Given an error activity
    const thread = makeThread({
      activities: [
        makeActivity({
          id: EventId.parse('error-1'),
          tone: 'error',
          kind: 'runtime.error',
          summary: 'Something failed',
          createdAt: '2026-04-01T00:00:01.000Z',
        }),
      ],
    });
    // When built
    const feed = buildThreadFeed(thread);
    // Then the activity surfaces as an error entry
    expect(feed[0]).toMatchObject({ kind: 'error', id: 'error-1' });
  });
});
