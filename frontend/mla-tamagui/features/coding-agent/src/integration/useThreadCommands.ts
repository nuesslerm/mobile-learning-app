import { useCallback } from 'react';
import { Logger } from '@lib/logging';
import {
  ApprovalRequestId,
  CommandId,
  EventId,
  MessageId,
  type ClientOrchestrationCommand,
  type InteractionMode,
  type ModelSelection,
  type OrchestrationCommandReceipt,
  type OrchestrationProposedPlan,
  type OrchestrationThread,
  type ProviderApprovalDecision,
  type RuntimeMode,
  type ThreadId,
} from '@app/remote-chat-agent-contracts/orchestration';

import type { OrchestrationGateway } from '../data/orchestrationGateway';
import {
  getDefaultInteractionMode,
  getDefaultRuntimeMode,
  useComposerStore,
} from '../state/composerStore';
import { useGatewayStore } from '../state/gatewayStore';
import { useOutboxStore } from '../state/outboxStore';
import { useThreadDetailStore } from '../state/threadDetailStore';
import { useThreadShellStore } from '../state/threadShellStore';

export type StartTurnOptions = {
  steer?: boolean;
  sourceProposedPlan?: unknown;
};

type ThreadBootstrapSource = {
  title: string;
  cwd: string;
  modelSelection: ModelSelection;
  runtimeMode: RuntimeMode;
  interactionMode: InteractionMode;
};

function fallbackUuid(): string {
  const random = (): string =>
    Math.floor(Math.random() * 0x100000000)
      .toString(16)
      .padStart(8, '0');
  const time = Date.now().toString(16).padStart(12, '0').slice(-12);
  const first = random();
  const second = random();
  const third = random();
  const fourth = random();
  return `${time.slice(0, 8)}-${time.slice(8)}-4${first.slice(0, 3)}-8${second.slice(0, 3)}-${third}${fourth.slice(0, 4)}`;
}

function nextUuid(): string {
  const randomUUID = globalThis.crypto?.randomUUID;
  if (typeof randomUUID === 'function') {
    return randomUUID.call(globalThis.crypto);
  }
  return fallbackUuid();
}

function nowIso(): string {
  return new Date().toISOString();
}

function queueCommand(
  command: ClientOrchestrationCommand,
): void {
  useOutboxStore.getState().enqueue({
    id: command.commandId,
    threadId: command.threadId,
    command,
    createdAt: Date.now(),
    attempts: 0,
  });
}

function commandDetail(
  command: ClientOrchestrationCommand,
): { requestId?: string; decision?: string } {
  if (command.type === 'thread.approval.respond') {
    return { requestId: command.requestId, decision: command.decision };
  }
  if (command.type === 'thread.user-input.respond') {
    return { requestId: command.requestId };
  }
  return {};
}

function errorCode(cause: unknown): string | undefined {
  const code = (cause as { code?: unknown } | null)?.code;
  return typeof code === 'string' ? code : undefined;
}

type CommandFailure = {
  reason: 'gateway-null' | 'rejected' | 'error';
  code?: string;
  gateway: OrchestrationGateway | null;
  startedAt: number;
};

// Every failure is queued in the outbox, so `queued` is always true.
function failureFields(
  command: ClientOrchestrationCommand,
  failure: CommandFailure,
): Record<string, unknown> {
  return {
    commandId: command.commandId,
    threadId: command.threadId,
    type: command.type,
    reason: failure.reason,
    code: failure.code,
    gatewayStatus: failure.gateway?.status(),
    queued: true,
    ms: Date.now() - failure.startedAt,
  };
}

function warnCommandFailure(
  command: ClientOrchestrationCommand,
  cause: unknown,
  failure: CommandFailure,
): void {
  Logger.withError(cause)
    .withMetadata(failureFields(command, failure))
    .warn('coding-agent.commands.dispatch_failed');
}

async function dispatchOrQueue(
  gateway: OrchestrationGateway | null,
  command: ClientOrchestrationCommand,
): Promise<OrchestrationCommandReceipt | undefined> {
  const startedAt = Date.now();
  if (gateway === null) {
    const cause = new Error('coding-agent gateway is unavailable');
    queueCommand(command);
    warnCommandFailure(command, cause, {
      reason: 'gateway-null',
      gateway,
      startedAt,
    });
    return undefined;
  }

  try {
    const receipt = await gateway.dispatch(command);
    if (receipt.status === 'rejected') {
      queueCommand(command);
      warnCommandFailure(
        command,
        new Error(receipt.error?.message ?? 'coding-agent command was rejected'),
        {
          reason: 'rejected',
          code: receipt.error?.code,
          gateway,
          startedAt,
        },
      );
    } else {
      Logger.info('coding-agent.commands.dispatched', {
        commandId: command.commandId,
        threadId: command.threadId,
        type: command.type,
        status: receipt.status,
        ms: Date.now() - startedAt,
        ...commandDetail(command),
      });
    }
    return receipt;
  } catch (cause) {
    queueCommand(command);
    Logger.withError(cause)
      .withMetadata(
        failureFields(command, {
          reason: 'error',
          code: errorCode(cause),
          gateway,
          startedAt,
        }),
      )
      .warn('coding-agent.commands.dispatch_failed');
    return undefined;
  }
}

function findThreadSource(threadId: ThreadId): ThreadBootstrapSource | undefined {
  const detail = useThreadDetailStore.getState().threads[threadId];
  if (detail !== undefined) {
    return {
      title: detail.title,
      cwd: detail.cwd,
      modelSelection: detail.modelSelection,
      runtimeMode: detail.runtimeMode,
      interactionMode: detail.interactionMode,
    };
  }

  const summary = useThreadShellStore
    .getState()
    .threads.find((thread) => thread.id === threadId);
  if (summary === undefined) return undefined;
  return {
    title: summary.title,
    cwd: summary.cwd,
    modelSelection: summary.modelSelection,
    runtimeMode: summary.runtimeMode,
    interactionMode: summary.interactionMode,
  };
}

function optimisticMessage(
  threadId: ThreadId,
  commandId: ClientOrchestrationCommand['commandId'],
  messageId: ReturnType<typeof MessageId.parse>,
  text: string,
  createdAt: string,
  source: ThreadBootstrapSource | undefined,
  runtimeMode: RuntimeMode,
  interactionMode: InteractionMode,
  modelSelection: ModelSelection,
): void {
  const store = useThreadDetailStore.getState();
  if (store.threads[threadId] === undefined) {
    const bootstrap = {
      title: source?.title ?? (text.trim().slice(0, 80) || 'New thread'),
      cwd: source?.cwd ?? '/',
      modelSelection,
      runtimeMode,
      interactionMode,
    };
    const snapshot: OrchestrationThread = {
      id: threadId,
      title: bootstrap.title,
      cwd: bootstrap.cwd,
      modelSelection: bootstrap.modelSelection,
      runtimeMode: bootstrap.runtimeMode,
      interactionMode: bootstrap.interactionMode,
      latestTurn: null,
      createdAt,
      updatedAt: createdAt,
      archivedAt: null,
      deletedAt: null,
      messages: [],
      proposedPlans: [],
      activities: [],
      session: null,
    };
    store.applyItem(threadId, { kind: 'snapshot', snapshot });
  }

  store.applyItem(threadId, {
    kind: 'event',
    event: {
      eventId: EventId.parse(nextUuid()),
      sequence: 1,
      createdAt,
      threadId,
      commandId,
      actor: 'client',
      type: 'thread.message-sent',
      payload: {
        message: {
          id: messageId,
          role: 'user',
          text,
          createdAt,
          updatedAt: createdAt,
        },
      },
    },
  });
}

function turnCommandValues(threadId: ThreadId): {
  modelSelection: ModelSelection;
  runtimeMode: RuntimeMode;
  interactionMode: InteractionMode;
} {
  const composer = useComposerStore.getState();
  const source = findThreadSource(threadId);
  return {
    modelSelection:
      composer.modelSelection[threadId] ??
      source?.modelSelection ??
      ({ cli: 'opencode' } satisfies ModelSelection),
    runtimeMode:
      composer.runtimeMode[threadId] ??
      source?.runtimeMode ??
      getDefaultRuntimeMode(threadId),
    interactionMode:
      composer.interactionMode[threadId] ??
      source?.interactionMode ??
      getDefaultInteractionMode(threadId),
  };
}

export function useThreadCommands(threadId: ThreadId) {
  const gateway = useGatewayStore((state) => state.gateway);

  const startTurn = useCallback(
    async (
      text: string,
      options: StartTurnOptions = {},
    ): Promise<OrchestrationCommandReceipt | undefined> => {
      if (text.trim().length === 0) return undefined;

      const thread = useThreadDetailStore.getState().threads[threadId];
      const isRunning =
        thread?.latestTurn?.state === 'running' ||
        thread?.session?.status === 'starting' ||
        thread?.session?.status === 'running';
      if (isRunning && options.steer !== true) {
        useComposerStore.getState().enqueueQueued(threadId, text);
        // dequeueQueued has no caller, so a held prompt is never sent: this line is its only trace.
        Logger.info('coding-agent.turn.queued_locally', {
          threadId,
          textLength: text.length,
          queueDepth: useComposerStore.getState().queued[threadId]?.length,
          turnState: thread?.latestTurn?.state,
          sessionStatus: thread?.session?.status,
        });
        return undefined;
      }

      const { modelSelection, runtimeMode, interactionMode } =
        turnCommandValues(threadId);
      const composerModelSelection =
        useComposerStore.getState().modelSelection[threadId];
      const commandId = CommandId.parse(nextUuid());
      const messageId = MessageId.parse(nextUuid());
      const createdAt = nowIso();
      const source = findThreadSource(threadId);
      const needsBootstrap = source === undefined;
      const command: ClientOrchestrationCommand = {
        type: 'thread.turn.start',
        commandId,
        threadId,
        message: {
          messageId,
          role: 'user',
          text,
        },
        ...(composerModelSelection !== null &&
        composerModelSelection !== undefined
          ? { modelSelection }
          : source === undefined
            ? { modelSelection }
            : {}),
        runtimeMode,
        interactionMode,
        ...(options.sourceProposedPlan !== undefined
          ? { sourceProposedPlan: options.sourceProposedPlan }
          : {}),
        ...(needsBootstrap
          ? {
              bootstrap: {
                createThread: {
                  title: text.trim().slice(0, 80) || 'New thread',
                  cwd: '/',
                  modelSelection,
                  runtimeMode,
                  interactionMode,
                },
              },
            }
          : {}),
        createdAt,
      };

      optimisticMessage(
        threadId,
        commandId,
        messageId,
        text,
        createdAt,
        source,
        runtimeMode,
        interactionMode,
        modelSelection,
      );
      // The prompt text is user content and never logged. A turn.start with no dispatched or dispatch_failed
      // for its commandId is a request that got no answer.
      Logger.info('coding-agent.turn.start', {
        threadId,
        commandId,
        textLength: text.length,
        steer: options.steer === true,
        bootstrap: needsBootstrap,
        runtimeMode,
        interactionMode,
        cli: modelSelection.cli,
      });
      return dispatchOrQueue(gateway, command);
    },
    [gateway, threadId],
  );

  const interrupt = useCallback(() => {
    const thread = useThreadDetailStore.getState().threads[threadId];
    const command: ClientOrchestrationCommand = {
      type: 'thread.turn.interrupt',
      commandId: CommandId.parse(nextUuid()),
      threadId,
      ...(thread?.latestTurn?.turnId !== undefined
        ? { turnId: thread.latestTurn.turnId }
        : {}),
      createdAt: nowIso(),
    };
    return dispatchOrQueue(gateway, command);
  }, [gateway, threadId]);

  const respondApproval = useCallback(
    (requestId: string, decision: ProviderApprovalDecision) => {
      const command: ClientOrchestrationCommand = {
        type: 'thread.approval.respond',
        commandId: CommandId.parse(nextUuid()),
        threadId,
        requestId: ApprovalRequestId.parse(requestId),
        decision,
        createdAt: nowIso(),
      };
      return dispatchOrQueue(gateway, command);
    },
    [gateway, threadId],
  );

  const respondUserInput = useCallback(
    (requestId: string, answers: Record<string, unknown>) => {
      const command: ClientOrchestrationCommand = {
        type: 'thread.user-input.respond',
        commandId: CommandId.parse(nextUuid()),
        threadId,
        requestId: ApprovalRequestId.parse(requestId),
        answers,
        createdAt: nowIso(),
      };
      return dispatchOrQueue(gateway, command);
    },
    [gateway, threadId],
  );

  const setModelSelection = useCallback(
    (selection: ModelSelection) => {
      useComposerStore.getState().setModelSelection(threadId, selection);
      const command: ClientOrchestrationCommand = {
        type: 'thread.meta.update',
        commandId: CommandId.parse(nextUuid()),
        threadId,
        modelSelection: selection,
      };
      return dispatchOrQueue(gateway, command);
    },
    [gateway, threadId],
  );

  const setRuntimeMode = useCallback(
    (mode: RuntimeMode) => {
      useComposerStore.getState().setRuntimeMode(threadId, mode);
      const command: ClientOrchestrationCommand = {
        type: 'thread.runtime-mode.set',
        commandId: CommandId.parse(nextUuid()),
        threadId,
        runtimeMode: mode,
        createdAt: nowIso(),
      };
      return dispatchOrQueue(gateway, command);
    },
    [gateway, threadId],
  );

  const setInteractionMode = useCallback(
    (mode: InteractionMode) => {
      useComposerStore.getState().setInteractionMode(threadId, mode);
      const command: ClientOrchestrationCommand = {
        type: 'thread.interaction-mode.set',
        commandId: CommandId.parse(nextUuid()),
        threadId,
        interactionMode: mode,
        createdAt: nowIso(),
      };
      return dispatchOrQueue(gateway, command);
    },
    [gateway, threadId],
  );

  const implementPlan = useCallback(
    (plan: OrchestrationProposedPlan): Promise<OrchestrationCommandReceipt | undefined> => {
      useComposerStore.getState().setInteractionMode(threadId, 'default');
      return startTurn('Implement the plan', { sourceProposedPlan: plan });
    },
    [startTurn, threadId],
  );

  const stopSession = useCallback(() => {
    const command: ClientOrchestrationCommand = {
      type: 'thread.session.stop',
      commandId: CommandId.parse(nextUuid()),
      threadId,
      createdAt: nowIso(),
    };
    return dispatchOrQueue(gateway, command);
  }, [gateway, threadId]);

  return {
    startTurn,
    interrupt,
    respondApproval,
    respondUserInput,
    setModelSelection,
    setRuntimeMode,
    setInteractionMode,
    implementPlan,
    stopSession,
  };
}
