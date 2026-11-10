// features/coding-agent/src/data/fixtureGateway.ts
//
// `OrchestrationGateway` implementation that drives the UI from
// `FIXTURE_THREADS` + `FIXTURE_SERVER_CONFIG` without any network IO.
// Used in tests, storybook proposals, the local-only "transport=fixture"
// entry point in the thread screen, and the future offline-first shell.
//
// `listThreads()` returns summaries derived from the fixtures.
// `subscribeThread()` emits a `synchronized` then a `snapshot` of the
// current in-memory thread, and for `fx-streaming` schedules text deltas
// every 400 ms until the assistant message reaches its scripted final text.
// `dispatch()` on a `thread.turn.start` whose `message.text` starts with
// `fixture:` switches the thread into a scripted reply
// (`stream | tool | diff | approval | question | plan | fail`); `approval`
// and `question` block on the matching `thread.approval.respond` /
// `thread.user-input.respond` command before resolving the pending activity
// and continuing with the rest of the script.
//
// Events are applied to the in-memory thread via the pure
// `applyThreadDetailEvent` reducer so `getThread` and the next snapshot
// reflect the same state the subscriber sees via stream items.
import { Logger } from '@lib/logging';
import {
  ApprovalRequestId,
  EventId,
  MessageId,
  ThreadId,
  TurnId,
  type ClientOrchestrationCommand,
  type OrchestrationCommandReceipt,
  type OrchestrationEvent,
  type OrchestrationProposedPlan,
  type OrchestrationSubscribeThreadItem,
  type OrchestrationThread,
  type OrchestrationThreadActivity,
  type OrchestrationThreadSummary,
  type ServerProbeResult,
  type ThreadApprovalRespondCommand,
  type ThreadTurnStartCommand,
  type ThreadUserInputRespondCommand,
} from '@app/remote-chat-agent-contracts/orchestration';
import {
  FIXTURE_SERVER_CONFIG,
  FIXTURE_THREADS,
  applyThreadDetailEvent,
  type FixtureId,
} from '../domain';

import type {
  OrchestrationGateway,
  OrchestrationGatewayStatus,
  OrchestrationShellListener,
  OrchestrationStatusListener,
  OrchestrationThreadListener,
} from './orchestrationGateway';

const STREAMING_TICK_MS = 400;
const FX_THREAD_PREFIX = 'fx-thread-';
const FX_STREAMING_ID = ThreadId.parse(FX_THREAD_PREFIX + 'fx-streaming');
const FX_ASSISTANT_MESSAGE_ID = MessageId.parse('fx-streaming-assistant-1');
const STREAMING_DELTAS: readonly string[] = [
  'Types flow through the code,\ngentle and strict,',
  'Types flow through the code,\ngentle and strict,\neach line a quiet promise.',
];

type ScriptedMode =
  | 'stream'
  | 'tool'
  | 'diff'
  | 'approval'
  | 'question'
  | 'plan'
  | 'fail';

type PendingApproval = {
  requestId: ApprovalRequestId;
  activityId: EventId;
  turnId: TurnId;
};

type PendingUserInput = {
  requestId: ApprovalRequestId;
  activityId: EventId;
  turnId: TurnId;
};

type FixtureState = {
  thread: OrchestrationThread;
  scripted: ScriptedMode | undefined;
  scriptedTurnId: TurnId | undefined;
  scriptedCancelled: boolean;
  pendingApprovals: Map<ApprovalRequestId, PendingApproval>;
  pendingUserInputs: Map<ApprovalRequestId, PendingUserInput>;
  nextSequence: number;
  nextActivityNumber: number;
};

function isThreadId(value: string): value is ThreadId {
  return ThreadId.safeParse(value).success;
}

function threadToSummary(thread: OrchestrationThread): OrchestrationThreadSummary {
  const {
    messages: _messages,
    activities: _activities,
    proposedPlans: _proposedPlans,
    ...summary
  } = thread;
  return summary;
}

function shallowCloneThread(thread: OrchestrationThread): OrchestrationThread {
  return {
    ...thread,
    modelSelection: { ...thread.modelSelection },
    latestTurn: thread.latestTurn === null ? null : { ...thread.latestTurn },
    messages: thread.messages.map((message) => ({ ...message })),
    proposedPlans: thread.proposedPlans.map((plan) => ({ ...plan })),
    activities: thread.activities.map((activity) => ({ ...activity })),
    session: thread.session === null ? null : { ...thread.session },
  };
}

function nextSequence(state: FixtureState): number {
  const candidate = state.nextSequence;
  state.nextSequence = candidate + 1;
  return candidate;
}

function nextActivityId(state: FixtureState): EventId {
  state.nextActivityNumber += 1;
  return EventId.parse(
    `fx-dispatch-${state.thread.id}-${state.nextActivityNumber}`,
  );
}

function fixtureToState(thread: OrchestrationThread): FixtureState {
  const maxSequence = thread.activities.reduce((max, activity) => {
    return activity.sequence === undefined
      ? max
      : Math.max(max, activity.sequence);
  }, -1);
  return {
    thread,
    scripted: undefined,
    scriptedTurnId: undefined,
    scriptedCancelled: false,
    pendingApprovals: new Map(),
    pendingUserInputs: new Map(),
    nextSequence: maxSequence + 1,
    nextActivityNumber: thread.activities.length,
  };
}

function initialFixtureStates(): Map<ThreadId, FixtureState> {
  const map = new Map<ThreadId, FixtureState>();
  for (const [id, thread] of Object.entries(FIXTURE_THREADS) as [
    FixtureId,
    OrchestrationThread,
  ][]) {
    const threadId = ThreadId.parse(FX_THREAD_PREFIX + id);
    map.set(threadId, fixtureToState({ ...thread, id: threadId }));
  }
  return map;
}

function parseScriptedMode(text: string): ScriptedMode | undefined {
  const match = /^fixture:(stream|tool|diff|approval|question|plan|fail)\b/.exec(
    text,
  );
  if (match === null) return undefined;
  switch (match[1]) {
    case 'stream':
      return 'stream';
    case 'tool':
      return 'tool';
    case 'diff':
      return 'diff';
    case 'approval':
      return 'approval';
    case 'question':
      return 'question';
    case 'plan':
      return 'plan';
    case 'fail':
      return 'fail';
    default:
      return undefined;
  }
}

export type FixtureGateway = OrchestrationGateway & {
  readonly recordedCommands: readonly ClientOrchestrationCommand[];
};

export function createFixtureGateway(): FixtureGateway {
  const states = initialFixtureStates();
  const threadSubscribers = new Map<
    ThreadId,
    Set<OrchestrationThreadListener>
  >();
  const shellSubscribers = new Set<OrchestrationShellListener>();
  const statusListeners = new Set<OrchestrationStatusListener>();
  const recordedCommands: ClientOrchestrationCommand[] = [];
  let status: OrchestrationGatewayStatus = 'open';
  let closed = false;

  const setStatus = (next: OrchestrationGatewayStatus): void => {
    if (status === next) return;
    status = next;
    for (const listener of statusListeners) {
      try {
        listener(next);
      } catch (cause) {
        Logger.withError(cause).warn(
          'coding-agent.fixture.status_listener_error',
        );
      }
    }
  };

  const emitToThreadListeners = (
    threadId: ThreadId,
    item: OrchestrationSubscribeThreadItem,
  ): void => {
    const listeners = threadSubscribers.get(threadId);
    if (listeners === undefined) return;
    for (const listener of listeners) {
      try {
        listener(item);
      } catch (cause) {
        Logger.withError(cause).withMetadata({ threadId }).warn(
          'coding-agent.fixture.thread_listener_error',
        );
      }
    }
  };

  const emitAssistantFinalMessage = (
    state: FixtureState,
    text: string,
  ): void => {
    const createdAt = new Date().toISOString();
    const messageId = MessageId.parse(
      `fx-final-${state.thread.id}-${state.nextSequence}`,
    );
    const event: OrchestrationEvent = {
      eventId: nextActivityId(state),
      sequence: nextSequence(state),
      createdAt,
      threadId: state.thread.id,
      actor: 'provider',
      type: 'thread.message-sent',
      payload: {
        message: {
          id: messageId,
          role: 'assistant',
          text,
          ...(state.scriptedTurnId !== undefined
            ? { turnId: state.scriptedTurnId }
            : {}),
          createdAt,
          updatedAt: createdAt,
        },
      },
    };
    state.thread = applyThreadDetailEvent(state.thread, event);
    emitToThreadListeners(state.thread.id, { kind: 'event', event });
  };

  const buildActivityEvent = (
    state: FixtureState,
    activity: OrchestrationThreadActivity,
  ): OrchestrationEvent => ({
    eventId: nextActivityId(state),
    sequence: nextSequence(state),
    createdAt: new Date().toISOString(),
    threadId: state.thread.id,
    actor: 'system',
    type: 'thread.activity-appended',
    payload: { activity },
  });

  const applyActivity = (
    state: FixtureState,
    activity: OrchestrationThreadActivity,
  ): void => {
    const event = buildActivityEvent(state, activity);
    state.thread = applyThreadDetailEvent(state.thread, event);
    emitToThreadListeners(state.thread.id, { kind: 'event', event });
  };

  const scheduleStreaming = (state: FixtureState): void => {
    if (state.scriptedCancelled) return;
    const threadId = state.thread.id;
    const assistantId = FX_ASSISTANT_MESSAGE_ID;
    for (const [index, delta] of STREAMING_DELTAS.entries()) {
      setTimeout(() => {
        if (state.scriptedCancelled) return;
        const createdAt = new Date().toISOString();
        const event: OrchestrationEvent = {
          eventId: nextActivityId(state),
          sequence: nextSequence(state),
          createdAt,
          threadId,
          actor: 'provider',
          type: 'thread.message-sent',
          payload: {
            message: {
              id: assistantId,
              role: 'assistant',
              text: delta,
              turnId: state.scriptedTurnId,
              createdAt,
              updatedAt: createdAt,
            },
          },
        };
        state.thread = applyThreadDetailEvent(state.thread, event);
        emitToThreadListeners(threadId, { kind: 'event', event });
      }, STREAMING_TICK_MS * (index + 1));
    }
  };

  const scheduleToolReply = (state: FixtureState): void => {
    setTimeout(() => {
      if (state.scriptedCancelled) return;
      applyActivity(state, {
        id: nextActivityId(state),
        tone: 'tool',
        kind: 'tool.completed',
        summary: 'pnpm install',
        payload: {
          itemType: 'command_execution',
          command: 'pnpm install',
          status: 'completed',
        },
        ...(state.scriptedTurnId !== undefined
          ? { turnId: state.scriptedTurnId }
          : {}),
        sequence: nextSequence(state),
        createdAt: new Date().toISOString(),
      });
    }, STREAMING_TICK_MS);
    setTimeout(() => {
      if (state.scriptedCancelled) return;
      applyActivity(state, {
        id: nextActivityId(state),
        tone: 'tool',
        kind: 'tool.completed',
        summary: 'Edit src/foo.ts',
        payload: {
          itemType: 'file_change',
          status: 'completed',
          data: { changedFiles: ['src/foo.ts'] },
        },
        ...(state.scriptedTurnId !== undefined
          ? { turnId: state.scriptedTurnId }
          : {}),
        sequence: nextSequence(state),
        createdAt: new Date().toISOString(),
      });
      emitAssistantFinalMessage(state, 'Patched src/foo.ts and re-ran the suite.');
    }, STREAMING_TICK_MS * 2);
  };

  const scheduleDiffReply = (state: FixtureState): void => {
    const threadId = state.thread.id;
    const unifiedDiff =
      'diff --git a/src/foo.ts b/src/foo.ts\n' +
      '--- a/src/foo.ts\n' +
      '+++ b/src/foo.ts\n' +
      '@@ -1,1 +1,1 @@\n' +
      '-export const a = 1;\n' +
      '+export const a = 2;\n';
    setTimeout(() => {
      if (state.scriptedCancelled) return;
      const turnId =
        state.scriptedTurnId ??
        TurnId.parse(`fx-turn-${state.thread.id}-diff`);
      const createdAt = new Date().toISOString();
      const event: OrchestrationEvent = {
        eventId: nextActivityId(state),
        sequence: nextSequence(state),
        createdAt,
        threadId,
        actor: 'provider',
        type: 'thread.turn-diff-updated',
        payload: { turnId, unifiedDiff },
      };
      state.thread = applyThreadDetailEvent(state.thread, event);
      emitToThreadListeners(threadId, { kind: 'event', event });
      emitAssistantFinalMessage(state, 'Updated the constant. Diff is shown above.');
    }, STREAMING_TICK_MS);
  };

  const scheduleApprovalRequest = (state: FixtureState): void => {
    setTimeout(() => {
      if (state.scriptedCancelled) return;
      const requestId = ApprovalRequestId.parse(
        `fx-req-${state.thread.id}-approval`,
      );
      const activityId = nextActivityId(state);
      const activity: OrchestrationThreadActivity = {
        id: activityId,
        tone: 'approval',
        kind: 'request.opened',
        summary: 'Approve patch',
        payload: {
          requestId,
          requestType: 'file_change_approval',
          detail: 'src/foo.ts',
        },
        ...(state.scriptedTurnId !== undefined
          ? { turnId: state.scriptedTurnId }
          : {}),
        sequence: nextSequence(state),
        createdAt: new Date().toISOString(),
      };
      applyActivity(state, activity);
      const turnId =
        state.scriptedTurnId ??
        state.thread.latestTurn?.turnId ??
        TurnId.parse(`fx-turn-${state.thread.id}-approval`);
      state.pendingApprovals.set(requestId, {
        requestId,
        activityId,
        turnId,
      });
    }, STREAMING_TICK_MS);
  };

  const scheduleUserInputRequest = (state: FixtureState): void => {
    setTimeout(() => {
      if (state.scriptedCancelled) return;
      const requestId = ApprovalRequestId.parse(
        `fx-req-${state.thread.id}-user-input`,
      );
      const activityId = nextActivityId(state);
      const activity: OrchestrationThreadActivity = {
        id: activityId,
        tone: 'approval',
        kind: 'user-input.requested',
        summary: 'Need input',
        payload: {
          requestId,
          questions: [
            {
              id: 'q-continue',
              header: 'Continue',
              question: 'Should I keep going?',
              options: [
                { label: 'Yes', description: 'Continue with the plan.' },
                { label: 'No', description: 'Stop here.' },
              ],
              multiSelect: false,
            },
          ],
        },
        ...(state.scriptedTurnId !== undefined
          ? { turnId: state.scriptedTurnId }
          : {}),
        sequence: nextSequence(state),
        createdAt: new Date().toISOString(),
      };
      applyActivity(state, activity);
      const turnId =
        state.scriptedTurnId ??
        state.thread.latestTurn?.turnId ??
        TurnId.parse(`fx-turn-${state.thread.id}-user-input`);
      state.pendingUserInputs.set(requestId, {
        requestId,
        activityId,
        turnId,
      });
    }, STREAMING_TICK_MS);
  };

  const schedulePlanReply = (state: FixtureState): void => {
    setTimeout(() => {
      if (state.scriptedCancelled) return;
      const createdAt = new Date().toISOString();
      const plan: OrchestrationProposedPlan = {
        id: `fx-plan-${state.thread.id}-${state.nextSequence}`,
        ...(state.scriptedTurnId !== undefined
          ? { turnId: state.scriptedTurnId }
          : {}),
        planMarkdown:
          '# Fixture plan\n\n' +
          '- [ ] Inspect the change.\n' +
          '- [ ] Apply the fix.\n' +
          '- [ ] Verify the result.\n',
        createdAt,
        updatedAt: createdAt,
      };
      const event: OrchestrationEvent = {
        eventId: nextActivityId(state),
        sequence: nextSequence(state),
        createdAt,
        threadId: state.thread.id,
        actor: 'provider',
        type: 'thread.proposed-plan-upserted',
        payload: { proposedPlan: plan },
      };
      state.thread = applyThreadDetailEvent(state.thread, event);
      emitToThreadListeners(state.thread.id, { kind: 'event', event });
      emitAssistantFinalMessage(state, 'Here is the plan — review and approve.');
    }, STREAMING_TICK_MS);
  };

  const scheduleFailReply = (state: FixtureState): void => {
    setTimeout(() => {
      if (state.scriptedCancelled) return;
      const createdAt = new Date().toISOString();
      const activity: OrchestrationThreadActivity = {
        id: nextActivityId(state),
        tone: 'error',
        kind: 'runtime.error',
        summary: 'Provider error',
        payload: {
          message: 'Provider returned 500 while streaming the response.',
          class: 'provider_error',
        },
        ...(state.scriptedTurnId !== undefined
          ? { turnId: state.scriptedTurnId }
          : {}),
        sequence: nextSequence(state),
        createdAt,
      };
      applyActivity(state, activity);
    }, STREAMING_TICK_MS);
  };

  const runScriptedReply = (state: FixtureState): void => {
    switch (state.scripted) {
      case 'stream':
        scheduleStreaming(state);
        return;
      case 'tool':
        scheduleToolReply(state);
        return;
      case 'diff':
        scheduleDiffReply(state);
        return;
      case 'approval':
        scheduleApprovalRequest(state);
        return;
      case 'question':
        scheduleUserInputRequest(state);
        return;
      case 'plan':
        schedulePlanReply(state);
        return;
      case 'fail':
        scheduleFailReply(state);
        return;
      case undefined:
        return;
    }
  };

  const dispatchTurnStart = (
    cmd: ThreadTurnStartCommand,
  ): OrchestrationCommandReceipt => {
    if (!isThreadId(cmd.threadId)) {
      return rejectedReceipt(cmd.commandId, 'invalid_thread_id');
    }
    const state = states.get(cmd.threadId);
    if (state === undefined) {
      return rejectedReceipt(cmd.commandId, 'thread_not_found');
    }

    const scripted = parseScriptedMode(cmd.message.text);
    state.scripted = scripted;
    state.scriptedCancelled = scripted === undefined;
    state.scriptedTurnId = TurnId.parse(
      `fx-turn-${cmd.threadId}-${state.nextSequence}`,
    );

    const createdAt = new Date().toISOString();
    const messageEvent: OrchestrationEvent = {
      eventId: nextActivityId(state),
      sequence: nextSequence(state),
      createdAt,
      threadId: cmd.threadId,
      commandId: cmd.commandId,
      actor: 'client',
      type: 'thread.message-sent',
      payload: {
        message: {
          id: cmd.message.messageId,
          role: 'user',
          text: cmd.message.text,
          turnId: state.scriptedTurnId,
          createdAt,
          updatedAt: createdAt,
        },
      },
    };
    const turnStartEvent: OrchestrationEvent = {
      eventId: nextActivityId(state),
      sequence: nextSequence(state),
      createdAt,
      threadId: cmd.threadId,
      commandId: cmd.commandId,
      actor: 'client',
      type: 'thread.turn-start-requested',
      payload: {
        messageId: cmd.message.messageId,
        ...(cmd.modelSelection !== undefined
          ? { modelSelection: cmd.modelSelection }
          : {}),
        runtimeMode: cmd.runtimeMode,
        interactionMode: cmd.interactionMode,
        createdAt,
      },
    };
    state.thread = applyThreadDetailEvent(state.thread, messageEvent);
    state.thread = applyThreadDetailEvent(state.thread, turnStartEvent);
    emitToThreadListeners(cmd.threadId, { kind: 'event', event: messageEvent });
    emitToThreadListeners(cmd.threadId, { kind: 'event', event: turnStartEvent });

    if (scripted !== undefined) {
      runScriptedReply(state);
    }

    return {
      commandId: cmd.commandId,
      status: 'accepted',
      events: [messageEvent, turnStartEvent],
    };
  };

  const dispatchApprovalRespond = (
    cmd: ThreadApprovalRespondCommand,
  ): OrchestrationCommandReceipt => {
    if (!isThreadId(cmd.threadId)) {
      return rejectedReceipt(cmd.commandId, 'invalid_thread_id');
    }
    const state = states.get(cmd.threadId);
    if (state === undefined) {
      return rejectedReceipt(cmd.commandId, 'thread_not_found');
    }
    const pending = state.pendingApprovals.get(cmd.requestId);
    if (pending === undefined) {
      return rejectedReceipt(cmd.commandId, 'request_not_pending');
    }
    state.pendingApprovals.delete(cmd.requestId);
    const createdAt = new Date().toISOString();
    applyActivity(state, {
      id: nextActivityId(state),
      tone: 'info',
      kind: 'request.resolved',
      summary: 'Approval resolved',
      payload: {
        requestId: cmd.requestId,
        requestType: 'file_change_approval',
        decision: cmd.decision,
      },
      ...(pending.turnId !== undefined ? { turnId: pending.turnId } : {}),
      sequence: nextSequence(state),
      createdAt,
    });
    if (state.scripted !== undefined) {
      emitAssistantFinalMessage(
        state,
        `Approval recorded (${cmd.decision}). Continuing.`,
      );
      state.scripted = undefined;
    }
    return {
      commandId: cmd.commandId,
      status: 'accepted',
      events: [],
    };
  };

  const dispatchUserInputRespond = (
    cmd: ThreadUserInputRespondCommand,
  ): OrchestrationCommandReceipt => {
    if (!isThreadId(cmd.threadId)) {
      return rejectedReceipt(cmd.commandId, 'invalid_thread_id');
    }
    const state = states.get(cmd.threadId);
    if (state === undefined) {
      return rejectedReceipt(cmd.commandId, 'thread_not_found');
    }
    const pending = state.pendingUserInputs.get(cmd.requestId);
    if (pending === undefined) {
      return rejectedReceipt(cmd.commandId, 'request_not_pending');
    }
    state.pendingUserInputs.delete(cmd.requestId);
    const createdAt = new Date().toISOString();
    applyActivity(state, {
      id: nextActivityId(state),
      tone: 'info',
      kind: 'user-input.resolved',
      summary: 'User input received',
      payload: { requestId: cmd.requestId, answers: cmd.answers },
      ...(pending.turnId !== undefined ? { turnId: pending.turnId } : {}),
      sequence: nextSequence(state),
      createdAt,
    });
    if (state.scripted !== undefined) {
      emitAssistantFinalMessage(state, 'Got it. Continuing with your answers.');
      state.scripted = undefined;
    }
    return {
      commandId: cmd.commandId,
      status: 'accepted',
      events: [],
    };
  };

  const dispatchOther = (
    cmd: ClientOrchestrationCommand,
  ): OrchestrationCommandReceipt => ({
    commandId: cmd.commandId,
    status: 'accepted',
    events: [],
  });

  const onShellListener = (cb: OrchestrationShellListener): (() => void) => {
    shellSubscribers.add(cb);
    const summaries: OrchestrationThreadSummary[] = Array.from(
      states.values(),
    ).map((state) => threadToSummary(state.thread));
    queueMicrotask(() => {
      if (!shellSubscribers.has(cb)) return;
      try {
        cb({ kind: 'synchronized' });
        cb({ kind: 'snapshot', snapshot: summaries });
      } catch (cause) {
        Logger.withError(cause).warn(
          'coding-agent.fixture.shell_listener_error',
        );
      }
    });
    return () => {
      shellSubscribers.delete(cb);
    };
  };

  const onThreadListener = (
    threadId: string,
    cb: OrchestrationThreadListener,
  ): (() => void) => {
    if (!isThreadId(threadId)) {
      queueMicrotask(() => {
        try {
          cb({ kind: 'synchronized' });
        } catch (cause) {
          Logger.withError(cause).warn(
            'coding-agent.fixture.thread_listener_error',
          );
        }
      });
      return () => undefined;
    }
    const state = states.get(threadId);
    if (state === undefined) {
      queueMicrotask(() => {
        try {
          cb({ kind: 'synchronized' });
        } catch (cause) {
          Logger.withError(cause).warn(
            'coding-agent.fixture.thread_listener_error',
          );
        }
      });
      return () => undefined;
    }
    const listeners =
      threadSubscribers.get(threadId) ??
      new Set<OrchestrationThreadListener>();
    listeners.add(cb);
    threadSubscribers.set(threadId, listeners);

    const snapshot = shallowCloneThread(state.thread);
    const isStreaming = threadId === FX_STREAMING_ID;
    queueMicrotask(() => {
      if (!listeners.has(cb)) return;
      try {
        cb({ kind: 'synchronized' });
        cb({ kind: 'snapshot', snapshot });
      } catch (cause) {
        Logger.withError(cause).warn(
          'coding-agent.fixture.thread_listener_error',
        );
      }
    });

    const timers: ReturnType<typeof setTimeout>[] = [];
    if (isStreaming) {
      const assistantId = FX_ASSISTANT_MESSAGE_ID;
      for (const [index, delta] of STREAMING_DELTAS.entries()) {
        const timer = setTimeout(() => {
          if (!listeners.has(cb)) return;
          const createdAt = new Date().toISOString();
          const event: OrchestrationEvent = {
            eventId: nextActivityId(state),
            sequence: nextSequence(state),
            createdAt,
            threadId,
            actor: 'provider',
            type: 'thread.message-sent',
            payload: {
              message: {
                id: assistantId,
                role: 'assistant',
                text: delta,
                turnId: snapshot.latestTurn?.turnId,
                createdAt,
                updatedAt: createdAt,
              },
            },
          };
          try {
            state.thread = applyThreadDetailEvent(state.thread, event);
            cb({ kind: 'event', event });
          } catch (cause) {
            Logger.withError(cause).warn(
              'coding-agent.fixture.thread_listener_error',
            );
          }
        }, STREAMING_TICK_MS * (index + 1));
        timers.push(timer);
      }
    }

    return () => {
      for (const timer of timers) {
        clearTimeout(timer);
      }
      if (listeners.delete(cb) && listeners.size === 0) {
        threadSubscribers.delete(threadId);
      }
    };
  };

  return {
    async connect(): Promise<void> {
      if (closed) {
        throw new Error('coding-agent.fixture gateway is closed');
      }
      setStatus('open');
    },
    close(): void {
      if (closed) return;
      closed = true;
      for (const state of states.values()) {
        state.scriptedCancelled = true;
      }
      threadSubscribers.clear();
      shellSubscribers.clear();
      statusListeners.clear();
      setStatus('closed');
    },
    status(): OrchestrationGatewayStatus {
      return status;
    },
    onStatus(cb: OrchestrationStatusListener): () => void {
      statusListeners.add(cb);
      try {
        cb(status);
      } catch (cause) {
        Logger.withError(cause).warn(
          'coding-agent.fixture.status_listener_error',
        );
      }
      return () => {
        statusListeners.delete(cb);
      };
    },
    async probe(): Promise<ServerProbeResult> {
      return FIXTURE_SERVER_CONFIG;
    },
    async listThreads(): Promise<OrchestrationThreadSummary[]> {
      return Array.from(states.values()).map((state) =>
        threadToSummary(state.thread),
      );
    },
    async getThread(id: string): Promise<OrchestrationThread> {
      if (!isThreadId(id)) {
        throw new Error(`getThread: invalid ThreadId ${id}`);
      }
      const state = states.get(id);
      if (state === undefined) {
        throw new Error(`getThread: thread ${id} not found`);
      }
      return shallowCloneThread(state.thread);
    },
    subscribeShell: onShellListener,
    subscribeThread: onThreadListener,
    async dispatch(
      cmd: ClientOrchestrationCommand,
    ): Promise<OrchestrationCommandReceipt> {
      if (closed) {
        throw new Error('coding-agent.fixture gateway is closed');
      }
      recordedCommands.push(cmd);
      switch (cmd.type) {
        case 'thread.turn.start':
          return dispatchTurnStart(cmd);
        case 'thread.approval.respond':
          return dispatchApprovalRespond(cmd);
        case 'thread.user-input.respond':
          return dispatchUserInputRespond(cmd);
        default:
          return dispatchOther(cmd);
      }
    },
    recordedCommands,
  };
}

function rejectedReceipt(
  commandId: ClientOrchestrationCommand['commandId'],
  code: string,
): OrchestrationCommandReceipt {
  return {
    commandId,
    status: 'rejected',
    events: [],
    error: { code, message: `dispatch rejected: ${code}` },
  };
}
