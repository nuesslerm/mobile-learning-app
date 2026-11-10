// features/coding-agent/src/domain/__tests__/threadReducer.test.ts
//
// Ported in structure from
// references/t3code/packages/client-runtime/src/state/threadReducer.test.ts
// onto our `OrchestrationThread` / `OrchestrationEvent` shapes. Every test is
// Given/When/Then; events are built with the `makeEvent(type, payload, seq)`
// helper (branded ids via `.parse`, as in the backend projector tests).

import {
  ApprovalRequestId,
  EventId,
  MessageId,
  ThreadId,
  TurnId,
  type OrchestrationEvent,
  type OrchestrationThread,
} from '@app/remote-chat-agent-contracts/orchestration';
import { describe, expect, it } from 'vitest';

import {
  applyThreadDetailEvent,
  type ThreadDetailState,
} from '../threadReducer';

const THREAD_ID = ThreadId.parse('thread-1');

function makeEvent(
  type: OrchestrationEvent['type'],
  payload: unknown,
  seq: number
): OrchestrationEvent {
  return {
    eventId: EventId.parse(`event-${seq}`),
    sequence: seq,
    createdAt: '2026-04-01T00:00:00.000Z',
    threadId: THREAD_ID,
    actor: 'client',
    type,
    payload,
  } as unknown as OrchestrationEvent;
}

function baseThread(): ThreadDetailState {
  return {
    id: THREAD_ID,
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
  };
}

describe('applyThreadDetailEvent', () => {
  describe('thread.created', () => {
    it('given no prior thread state, when thread.created is applied, then a fresh thread is created with empty collections', () => {
      // Given a base thread and a thread.created event
      const event = makeEvent(
        'thread.created',
        {
          title: 'New Thread',
          cwd: '/repo',
          modelSelection: { cli: 'codex', modelID: 'gpt-5.4' },
          runtimeMode: 'full-access',
          interactionMode: 'default',
          branch: 'main',
          createdAt: '2026-04-01T01:00:00.000Z',
        },
        1
      );
      // When the event is applied
      const result = applyThreadDetailEvent(baseThread(), event);
      // Then a fresh thread exists with empty collections
      expect(result.id).toBe(THREAD_ID);
      expect(result.title).toBe('New Thread');
      expect(result.branch).toBe('main');
      expect(result.messages).toEqual([]);
      expect(result.session).toBeNull();
      expect(result.turnDiffs).toEqual({});
    });
  });

  describe('thread.deleted', () => {
    it('given an existing thread, when thread.deleted is applied, then deletedAt is set', () => {
      // Given an existing thread
      const deletedAt = '2026-04-01T02:00:00.000Z';
      const event = makeEvent('thread.deleted', { deletedAt }, 2);
      // When the event is applied
      const result = applyThreadDetailEvent(baseThread(), event);
      // Then deletedAt is set
      expect(result.deletedAt).toBe(deletedAt);
    });
  });

  describe('thread.archived / thread.unarchived', () => {
    it('given an existing thread, when thread.archived is applied, then archivedAt is set', () => {
      // Given an existing thread
      const archivedAt = '2026-04-01T03:00:00.000Z';
      const event = makeEvent(
        'thread.archived',
        { archivedAt, updatedAt: archivedAt },
        3
      );
      // When the event is applied
      const result = applyThreadDetailEvent(baseThread(), event);
      // Then archivedAt is set
      expect(result.archivedAt).toBe(archivedAt);
    });

    it('given an archived thread, when thread.unarchived is applied, then archivedAt is cleared', () => {
      // Given an archived thread
      const updatedAt = '2026-04-01T04:00:00.000Z';
      const event = makeEvent('thread.unarchived', { updatedAt }, 4);
      // When the event is applied
      const result = applyThreadDetailEvent(
        { ...baseThread(), archivedAt: '2026-04-01T03:00:00.000Z' },
        event
      );
      // Then archivedAt is null
      expect(result.archivedAt).toBeNull();
    });
  });

  describe('thread.meta-updated', () => {
    it('given an existing thread, when thread.meta-updated is applied, then title and branch are patched and modelSelection is unchanged', () => {
      // Given an existing thread
      const event = makeEvent(
        'thread.meta-updated',
        {
          title: 'Updated Title',
          branch: 'feature/demo',
          updatedAt: '2026-04-01T05:00:00.000Z',
        },
        5
      );
      // When the event is applied
      const result = applyThreadDetailEvent(baseThread(), event);
      // Then title and branch are patched and modelSelection is unchanged
      expect(result.title).toBe('Updated Title');
      expect(result.branch).toBe('feature/demo');
      expect(result.modelSelection).toEqual(baseThread().modelSelection);
    });
  });

  describe('thread.runtime-mode-set / thread.interaction-mode-set', () => {
    it('given an existing thread, when thread.runtime-mode-set is applied, then runtimeMode is updated', () => {
      // Given an existing thread
      const event = makeEvent(
        'thread.runtime-mode-set',
        { runtimeMode: 'auto', updatedAt: '2026-04-01T05:00:00.000Z' },
        6
      );
      // When the event is applied
      const result = applyThreadDetailEvent(baseThread(), event);
      // Then runtimeMode is updated
      expect(result.runtimeMode).toBe('auto');
    });

    it('given an existing thread, when thread.interaction-mode-set is applied, then interactionMode is updated', () => {
      // Given an existing thread
      const event = makeEvent(
        'thread.interaction-mode-set',
        { interactionMode: 'plan', updatedAt: '2026-04-01T05:00:00.000Z' },
        7
      );
      // When the event is applied
      const result = applyThreadDetailEvent(baseThread(), event);
      // Then interactionMode is updated
      expect(result.interactionMode).toBe('plan');
    });
  });

  describe('thread.message-sent', () => {
    it('given a thread with no messages, when thread.message-sent is applied, then a new message is appended', () => {
      // Given a thread with no messages
      const event = makeEvent(
        'thread.message-sent',
        {
          message: {
            id: MessageId.parse('msg-1'),
            role: 'user',
            text: 'Hello, world!',
            turnId: TurnId.parse('turn-1'),
            createdAt: '2026-04-01T06:00:00.000Z',
            updatedAt: '2026-04-01T06:00:00.000Z',
          },
        },
        8
      );
      // When the event is applied
      const result = applyThreadDetailEvent(baseThread(), event);
      // Then a new message is appended
      expect(result.messages).toHaveLength(1);
      expect(result.messages[0]?.text).toBe('Hello, world!');
    });

    it('given a thread that already has msg-1, when thread.message-sent for the same id is applied, then the message is replaced, not duplicated', () => {
      // Given a thread that already has msg-1
      const existing: OrchestrationThread = {
        ...baseThread(),
        messages: [
          {
            id: MessageId.parse('msg-1'),
            role: 'user',
            text: 'Old text',
            createdAt: '2026-04-01T06:00:00.000Z',
            updatedAt: '2026-04-01T06:00:00.000Z',
          },
        ],
      };
      const event = makeEvent(
        'thread.message-sent',
        {
          message: {
            id: MessageId.parse('msg-1'),
            role: 'user',
            text: 'New text',
            createdAt: '2026-04-01T06:00:00.000Z',
            updatedAt: '2026-04-01T06:01:00.000Z',
          },
        },
        9
      );
      // When the event is applied
      const result = applyThreadDetailEvent(existing, event);
      // Then the message is replaced, not duplicated
      expect(result.messages).toHaveLength(1);
      expect(result.messages[0]?.text).toBe('New text');
    });
  });

  describe('thread.turn-start-requested', () => {
    it('given an existing thread, when thread.turn-start-requested is applied, then runtimeMode and interactionMode are updated', () => {
      // Given an existing thread
      const event = makeEvent(
        'thread.turn-start-requested',
        {
          messageId: MessageId.parse('msg-1'),
          runtimeMode: 'auto',
          interactionMode: 'plan',
          createdAt: '2026-04-01T06:00:00.000Z',
        },
        10
      );
      // When the event is applied
      const result = applyThreadDetailEvent(baseThread(), event);
      // Then runtimeMode and interactionMode are updated
      expect(result.runtimeMode).toBe('auto');
      expect(result.interactionMode).toBe('plan');
    });
  });

  describe('thread.turn-interrupt-requested', () => {
    it('given a thread whose latestTurn is running for turn-1, when thread.turn-interrupt-requested is applied, then the latestTurn becomes interrupted', () => {
      // Given a thread whose latestTurn is running for turn-1
      const runningThread: OrchestrationThread = {
        ...baseThread(),
        latestTurn: {
          turnId: TurnId.parse('turn-1'),
          state: 'running',
          requestedAt: '2026-04-01T06:00:00.000Z',
          startedAt: '2026-04-01T06:00:00.000Z',
          completedAt: null,
        },
      };
      const event = makeEvent(
        'thread.turn-interrupt-requested',
        {
          turnId: TurnId.parse('turn-1'),
          createdAt: '2026-04-01T07:00:00.000Z',
        },
        11
      );
      // When the event is applied
      const result = applyThreadDetailEvent(runningThread, event);
      // Then the latestTurn becomes interrupted
      expect(result.latestTurn?.state).toBe('interrupted');
    });

    it('given a thread whose latestTurn is for turn-1, when thread.turn-interrupt-requested for a different turn is applied, then the thread is unchanged', () => {
      // Given a thread whose latestTurn is for turn-1
      const runningThread: OrchestrationThread = {
        ...baseThread(),
        latestTurn: {
          turnId: TurnId.parse('turn-1'),
          state: 'running',
          requestedAt: '2026-04-01T06:00:00.000Z',
          startedAt: '2026-04-01T06:00:00.000Z',
          completedAt: null,
        },
      };
      const event = makeEvent(
        'thread.turn-interrupt-requested',
        {
          turnId: TurnId.parse('turn-2'),
          createdAt: '2026-04-01T07:00:00.000Z',
        },
        12
      );
      // When the event is applied
      const result = applyThreadDetailEvent(runningThread, event);
      // Then the thread is unchanged
      expect(result).toBe(runningThread);
    });
  });

  describe('thread.session-set', () => {
    it('given a thread whose latestTurn is running, when a ready session is set, then the latestTurn is completed', () => {
      // Given a thread whose latestTurn is running
      const runningTurn: OrchestrationThread = {
        ...baseThread(),
        latestTurn: {
          turnId: TurnId.parse('turn-1'),
          state: 'running',
          requestedAt: '2026-04-01T07:00:00.000Z',
          startedAt: '2026-04-01T07:00:00.000Z',
          completedAt: null,
        },
      };
      const event = makeEvent(
        'thread.session-set',
        {
          session: { cli: 'claude', status: 'ready', activeTurnId: undefined },
        },
        13
      );
      // When the event is applied
      const result = applyThreadDetailEvent(runningTurn, event);
      // Then the latestTurn is completed
      expect(result.latestTurn?.state).toBe('completed');
      expect(result.latestTurn?.completedAt).toBe('2026-04-01T00:00:00.000Z');
    });

    it('given an existing thread, when a running session is set, then the session and latestTurn are running', () => {
      // Given an existing thread
      const event = makeEvent(
        'thread.session-set',
        {
          session: {
            cli: 'codex',
            status: 'running',
            activeTurnId: TurnId.parse('turn-1'),
          },
        },
        14
      );
      // When the event is applied
      const result = applyThreadDetailEvent(baseThread(), event);
      // Then the session is running and latestTurn is running
      expect(result.session?.status).toBe('running');
      expect(result.latestTurn?.turnId).toBe(TurnId.parse('turn-1'));
      expect(result.latestTurn?.state).toBe('running');
    });
  });

  describe('thread.session-stop-requested', () => {
    it('given a thread with a running session, when thread.session-stop-requested is applied, then the session is stopped with no active turn', () => {
      // Given a thread with a running session
      const threadWithSession: OrchestrationThread = {
        ...baseThread(),
        session: {
          cli: 'codex',
          status: 'running',
          activeTurnId: TurnId.parse('turn-1'),
        },
      };
      const event = makeEvent(
        'thread.session-stop-requested',
        { createdAt: '2026-04-01T09:00:00.000Z' },
        15
      );
      // When the event is applied
      const result = applyThreadDetailEvent(threadWithSession, event);
      // Then the session is stopped with no active turn
      expect(result.session?.status).toBe('stopped');
      expect(result.session?.activeTurnId).toBeUndefined();
    });

    it('given a thread with no session, when thread.session-stop-requested is applied, then the thread is unchanged', () => {
      // Given a thread with no session
      const thread = baseThread();
      const event = makeEvent(
        'thread.session-stop-requested',
        { createdAt: '2026-04-01T09:00:00.000Z' },
        16
      );
      // When the event is applied
      const result = applyThreadDetailEvent(thread, event);
      // Then the thread is unchanged
      expect(result).toBe(thread);
    });
  });

  describe('thread.proposed-plan-upserted', () => {
    it('given an existing thread, when thread.proposed-plan-upserted is applied, then a proposed plan is added sorted by createdAt then id', () => {
      // Given an existing thread
      const event = makeEvent(
        'thread.proposed-plan-upserted',
        {
          proposedPlan: {
            id: 'plan-1',
            turnId: TurnId.parse('turn-1'),
            planMarkdown: '## Plan\n- Do stuff',
            createdAt: '2026-04-01T10:00:00.000Z',
            updatedAt: '2026-04-01T10:00:00.000Z',
          },
        },
        17
      );
      // When the event is applied
      const result = applyThreadDetailEvent(baseThread(), event);
      // Then a proposed plan is added
      expect(result.proposedPlans).toHaveLength(1);
      expect(result.proposedPlans[0]?.id).toBe('plan-1');
    });
  });

  describe('thread.activity-appended', () => {
    it('given an existing thread, when thread.activity-appended is applied, then an activity is added sorted by createdAt then id', () => {
      // Given an existing thread
      const event = makeEvent(
        'thread.activity-appended',
        {
          activity: {
            id: EventId.parse('activity-1'),
            tone: 'tool',
            kind: 'file-edit',
            summary: 'Edited src/index.ts',
            payload: {},
            turnId: TurnId.parse('turn-1'),
            createdAt: '2026-04-01T11:00:00.000Z',
          },
        },
        18
      );
      // When the event is applied
      const result = applyThreadDetailEvent(baseThread(), event);
      // Then an activity is added
      expect(result.activities).toHaveLength(1);
      expect(result.activities[0]?.kind).toBe('file-edit');
    });

    it('given a thread that already has activity-1, when thread.activity-appended for the same id is applied, then the activity is replaced, not duplicated', () => {
      // Given a thread that already has activity-1
      const existing: OrchestrationThread = {
        ...baseThread(),
        activities: [
          {
            id: EventId.parse('activity-1'),
            tone: 'info',
            kind: 'command',
            summary: 'Old summary',
            payload: {},
            turnId: TurnId.parse('turn-1'),
            createdAt: '2026-04-01T11:00:00.000Z',
          },
        ],
      };
      const event = makeEvent(
        'thread.activity-appended',
        {
          activity: {
            id: EventId.parse('activity-1'),
            tone: 'tool',
            kind: 'file-edit',
            summary: 'New summary',
            payload: {},
            turnId: TurnId.parse('turn-1'),
            createdAt: '2026-04-01T11:01:00.000Z',
          },
        },
        19
      );
      // When the event is applied
      const result = applyThreadDetailEvent(existing, event);
      // Then the activity is replaced, not duplicated
      expect(result.activities).toHaveLength(1);
      expect(result.activities[0]?.kind).toBe('file-edit');
    });
  });

  describe('thread.turn-diff-updated', () => {
    it('given an existing thread, when thread.turn-diff-updated is applied, then the diff is stored on the turnDiffs map', () => {
      // Given an existing thread
      const event = makeEvent(
        'thread.turn-diff-updated',
        {
          turnId: TurnId.parse('turn-1'),
          unifiedDiff: '--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n',
        },
        20
      );
      // When the event is applied
      const result = applyThreadDetailEvent(baseThread(), event);
      // Then the diff is stored on the turnDiffs map
      expect(result.turnDiffs?.['turn-1']).toContain('+new');
    });
  });

  describe('no-op events', () => {
    it('given an existing thread, when thread.approval-response-requested is applied, then the thread is unchanged', () => {
      // Given an existing thread
      const thread = baseThread();
      const event = makeEvent(
        'thread.approval-response-requested',
        {
          requestId: ApprovalRequestId.parse('req-1'),
          decision: 'accept',
          createdAt: '2026-04-01T13:00:00.000Z',
        },
        21
      );
      // When the event is applied
      const result = applyThreadDetailEvent(thread, event);
      // Then the thread is unchanged
      expect(result).toBe(thread);
    });

    it('given an existing thread, when thread.user-input-response-requested is applied, then the thread is unchanged', () => {
      // Given an existing thread
      const thread = baseThread();
      const event = makeEvent(
        'thread.user-input-response-requested',
        {
          requestId: ApprovalRequestId.parse('req-1'),
          answers: {},
          createdAt: '2026-04-01T13:00:00.000Z',
        },
        22
      );
      // When the event is applied
      const result = applyThreadDetailEvent(thread, event);
      // Then the thread is unchanged
      expect(result).toBe(thread);
    });
  });

  describe('unknown events', () => {
    it('given an existing thread, when an unrecognized event type is applied, then the thread is unchanged', () => {
      // Given an existing thread
      const thread = baseThread();
      const event = {
        ...makeEvent(
          'thread.created',
          {
            title: 'x',
            cwd: '/',
            modelSelection: { cli: 'codex' },
            runtimeMode: 'auto',
            interactionMode: 'default',
            createdAt: '2026-04-01T00:00:00.000Z',
          },
          23
        ),
        type: 'thread.mystery',
      } as unknown as OrchestrationEvent;
      // When the event is applied
      const result = applyThreadDetailEvent(thread, event);
      // Then the thread is unchanged
      expect(result).toBe(thread);
    });
  });
});
