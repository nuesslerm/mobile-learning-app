import {
  EventId,
  MessageId,
  ThreadId,
  type OrchestrationEvent,
  type OrchestrationSession,
  type OrchestrationThread,
} from '@app/remote-chat-agent-contracts/orchestration';

export const THREAD_ID = ThreadId.parse('thread-1');

const at = (second: number): string =>
  new Date(Date.UTC(2026, 3, 1, 0, 0, second)).toISOString();

export function makeThread(
  overrides: Partial<OrchestrationThread> = {}
): OrchestrationThread {
  return {
    id: THREAD_ID,
    title: 'Test Thread',
    cwd: '/repo',
    modelSelection: { cli: 'opencode', modelID: 'gpt-5.4' },
    runtimeMode: 'full-access',
    interactionMode: 'default',
    latestTurn: null,
    createdAt: at(0),
    updatedAt: at(0),
    archivedAt: null,
    deletedAt: null,
    messages: [],
    proposedPlans: [],
    activities: [],
    session: null,
    ...overrides,
  };
}

export function sessionSetEvent(
  sequence: number,
  session: OrchestrationSession
): OrchestrationEvent {
  return {
    eventId: EventId.parse(`event-${sequence}`),
    sequence,
    createdAt: at(sequence),
    threadId: THREAD_ID,
    actor: 'provider',
    type: 'thread.session-set',
    payload: { session },
  } as OrchestrationEvent;
}

export function messageSentEvent(sequence: number): OrchestrationEvent {
  return {
    eventId: EventId.parse(`event-${sequence}`),
    sequence,
    createdAt: at(sequence),
    threadId: THREAD_ID,
    actor: 'provider',
    type: 'thread.message-sent',
    payload: {
      message: {
        id: MessageId.parse(`message-${sequence}`),
        role: 'assistant',
        text: 'hello',
        createdAt: at(sequence),
        updatedAt: at(sequence),
      },
    },
  } as OrchestrationEvent;
}
