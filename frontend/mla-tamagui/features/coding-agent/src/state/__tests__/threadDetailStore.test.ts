// features/coding-agent/src/state/__tests__/threadDetailStore.test.ts
//
// State test for `useThreadDetailStore` with an injected fake gateway
// (`FakeGateway`), in the Given/When/Then style the domain tests use. The
// fake records dispatched commands, throws when told it is disconnected, and
// fans pushed stream items out to subscribers, so these tests cover the store
// feed/latestTurn fold, the approval.respond command shape the feature
// dispatches, and the outbox queueing a failed dispatch leaves behind.
import {
  ApprovalRequestId,
  CommandId,
  EventId,
  MessageId,
  ThreadId,
  TurnId,
  type ClientOrchestrationCommand,
  type OrchestrationEvent,
  type OrchestrationThread,
  type ThreadApprovalRespondCommand,
  type ThreadTurnStartCommand,
} from '@app/remote-chat-agent-contracts/orchestration';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { useOutboxStore } from '../outboxStore';
import { useThreadDetailStore } from '../threadDetailStore';
import { FakeGateway } from './fakeGateway';

// The `src/domain` barrel re-exports `./fixtures`, whose
// `threads.ts` parses every fixture through `OrchestrationThreadSchema` at
// module load — and `OrchestrationSessionSchema.activeTurnId` is
// `TurnId.optional()` (rejects `null`) while the fixtures pass `activeTurnId:
// null`, so importing the barrel throws before any test runs. The fixtures
// are not needed here, so mock the barrel and re-export the real domain
// modules by their direct file paths (none of them import `./fixtures`).
vi.mock('../../domain', async () => {
  const threadReducer = await import(
    '../../domain/threadReducer'
  );
  const threadFeed = await import(
    '../../domain/threadFeed'
  );
  const pendingRequests = await import(
    '../../domain/pendingRequests'
  );
  const workLog = await import('../../domain/workLog');
  const outbox = await import('../../domain/outbox');
  return {
    applyThreadDetailEvent: threadReducer.applyThreadDetailEvent,
    buildThreadFeed: threadFeed.buildThreadFeed,
    derivePendingApprovals: pendingRequests.derivePendingApprovals,
    derivePendingUserInputs: pendingRequests.derivePendingUserInputs,
    deriveThreadFeedPresentation: workLog.deriveThreadFeedPresentation,
    enqueue: outbox.enqueue,
    markAttempt: outbox.markAttempt,
    remove: outbox.remove,
    OutboxItemSchema: outbox.OutboxItemSchema,
  };
});

// `outboxStorage` reaches for the real AsyncStorage package on import; the
// node test environment has no native storage backend, so stub it with an
// in-memory map like the leetcode auth store test does.
vi.mock('@react-native-async-storage/async-storage', () => {
  const store = new Map<string, string>();
  return {
    default: {
      getItem: async (key: string) => store.get(key) ?? null,
      setItem: async (key: string, value: string) => {
        store.set(key, value);
      },
      removeItem: async (key: string) => {
        store.delete(key);
      },
    },
  };
});

const THREAD_ID = ThreadId.parse('thread-1');

function makeThread(overrides: Partial<OrchestrationThread> = {}): OrchestrationThread {
  return {
    id: THREAD_ID,
    title: 'Test Thread',
    cwd: '/repo',
    modelSelection: { cli: 'opencode', modelID: 'gpt-5.4' },
    runtimeMode: 'full-access',
    interactionMode: 'default',
    latestTurn: {
      turnId: TurnId.parse('turn-1'),
      state: 'running',
      requestedAt: '2026-04-01T00:00:00.000Z',
      startedAt: '2026-04-01T00:00:00.000Z',
      completedAt: null,
    },
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

function makeEvent(
  overrides: Partial<OrchestrationEvent>,
): OrchestrationEvent {
  return {
    eventId: EventId.parse('event-1'),
    sequence: 1,
    createdAt: '2026-04-01T00:00:00.000Z',
    threadId: THREAD_ID,
    actor: 'provider',
    ...overrides,
  } as OrchestrationEvent;
}

function subscribe(fake: FakeGateway): void {
  fake.subscribeThread(THREAD_ID, (item) => {
    useThreadDetailStore.getState().applyItem(THREAD_ID, item);
  });
}

describe('threadDetailStore with injected fake gateway', () => {
  beforeEach(() => {
    useThreadDetailStore.setState({ threads: {}, expandedGroupIds: new Set() });
    useOutboxStore.setState({ items: [], hydrated: false });
  });

  it('given a subscribed thread with a startTurn, when the fake streams snapshot → message deltas → completed, then the feed has 1 user + 1 assistant entry and latestTurn completed', () => {
    // Given a subscribed thread whose startTurn produced an optimistic user message
    const fake = new FakeGateway();
    subscribe(fake);
    const snapshot = makeThread({
      latestTurn: {
        turnId: TurnId.parse('turn-1'),
        state: 'running',
        requestedAt: '2026-04-01T00:00:00.000Z',
        startedAt: '2026-04-01T00:00:00.000Z',
        completedAt: null,
      },
      messages: [
        {
          id: MessageId.parse('user-1'),
          role: 'user',
          text: 'Say hello',
          createdAt: '2026-04-01T00:00:00.000Z',
          updatedAt: '2026-04-01T00:00:00.000Z',
        },
      ],
    });
    fake.push(THREAD_ID, { kind: 'snapshot', snapshot });

    // When the fake streams assistant deltas then marks the turn completed
    fake.push(THREAD_ID, {
      kind: 'event',
      event: makeEvent({
        eventId: EventId.parse('assistant-1'),
        sequence: 2,
        createdAt: '2026-04-01T00:00:01.000Z',
        type: 'thread.message-sent',
        payload: {
          message: {
            id: MessageId.parse('assistant-1'),
            role: 'assistant',
            text: 'Hello there',
            turnId: TurnId.parse('turn-1'),
            createdAt: '2026-04-01T00:00:01.000Z',
            updatedAt: '2026-04-01T00:00:01.000Z',
          },
        },
      }),
    });
    fake.push(THREAD_ID, {
      kind: 'event',
      event: makeEvent({
        eventId: EventId.parse('session-set-1'),
        sequence: 3,
        createdAt: '2026-04-01T00:00:02.000Z',
        type: 'thread.session-set',
        payload: {
          session: {
            cli: 'opencode',
            providerSessionId: 'session-1',
            status: 'ready',
            activeTurnId: undefined,
          },
        },
      }),
    });

    // Then the feed has one user and one assistant entry and the turn completed
    const feed = useThreadDetailStore.getState().feed(THREAD_ID);
    expect(feed.filter((entry) => entry.kind === 'user-message')).toHaveLength(1);
    expect(
      feed.filter((entry) => entry.kind === 'assistant-message'),
    ).toHaveLength(1);
    expect(useThreadDetailStore.getState().getThread(THREAD_ID)?.latestTurn).toMatchObject(
      { state: 'completed' },
    );
  });

  it('given an approval.requested activity, when respondApproval(accept) dispatches, then the fake received thread.approval.respond with decision accept', async () => {
    // Given a subscribed thread whose snapshot carries an approval.requested activity
    const fake = new FakeGateway();
    subscribe(fake);
    fake.push(THREAD_ID, {
      kind: 'snapshot',
      snapshot: makeThread({
        activities: [
          {
            id: EventId.parse('approval-1'),
            tone: 'approval',
            kind: 'request.opened',
            summary: 'Approve patch',
            payload: {
              requestId: ApprovalRequestId.parse('req-1'),
              requestType: 'file_change_approval',
              detail: 'src/foo.ts',
            },
            turnId: TurnId.parse('turn-1'),
            sequence: 0,
            createdAt: '2026-04-01T00:00:01.000Z',
          },
        ],
      }),
    });

    // When respondApproval('accept') dispatches the command
    const command: ThreadApprovalRespondCommand = {
      type: 'thread.approval.respond',
      commandId: CommandId.parse('cmd-approval'),
      threadId: THREAD_ID,
      requestId: ApprovalRequestId.parse('req-1'),
      decision: 'accept',
      createdAt: '2026-04-01T00:00:02.000Z',
    };
    await fake.dispatch(command);

    // Then the fake received thread.approval.respond with decision accept
    expect(fake.dispatched).toHaveLength(1);
    expect(fake.dispatched[0]).toMatchObject({
      type: 'thread.approval.respond',
      decision: 'accept',
    });
  });

  it('given a disconnected gateway, when startTurn dispatch fails, then the command sits in the outbox', async () => {
    // Given a disconnected gateway and a turn.start command
    const fake = new FakeGateway();
    fake.disconnected = true;
    const command: ClientOrchestrationCommand = {
      type: 'thread.turn.start',
      commandId: CommandId.parse('cmd-start'),
      threadId: THREAD_ID,
      message: {
        messageId: MessageId.parse('user-1'),
        role: 'user',
        text: 'Say hello',
      },
      modelSelection: { cli: 'opencode', modelID: 'gpt-5.4' },
      runtimeMode: 'full-access',
      interactionMode: 'default',
      createdAt: '2026-04-01T00:00:00.000Z',
    } satisfies ThreadTurnStartCommand;

    // When startTurn fails (dispatch rejects)
    await expect(fake.dispatch(command)).rejects.toThrow();
    useOutboxStore.getState().enqueue({
      id: command.commandId,
      threadId: THREAD_ID,
      command,
      createdAt: Date.now(),
      attempts: 0,
    });

    // Then the command sits in the outbox
    const items = useOutboxStore.getState().items;
    expect(items).toHaveLength(1);
    expect(items[0].command).toEqual(command);
  });
});
