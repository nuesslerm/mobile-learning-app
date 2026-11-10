// features/coding-agent/src/state/threadShellStore.ts
//
// Zustand store for the shell list of threads: a cached
// `OrchestrationThreadSummary[]` plus the actions that drive it. The
// `applyShellItem` reducer projects a single `OrchestrationSubscribeShellItem`
// (snapshot, synchronized marker, or a per-thread event) into the cached
// list — summary-only fields are folded in place; events that don't touch
// summary fields are no-ops, so the projection is total over the full
// event union. `refresh(gateway)` re-issues `orchestration.listThreads`
// from scratch (used on cold start and after a reconnect), and
// `createThread(gateway, input)` dispatches a `thread.create` command
// and lets the shell subscription pick the new thread up via the
// resulting `thread.created` event so the projection and the wire stay
// in sync without a separate optimistic-update path.
import { Logger } from '@lib/logging';
import type { ModelSelection } from '../domain';
import {
  CommandId,
  InteractionMode,
  RuntimeMode,
  ThreadId,
  type OrchestrationCommandReceipt,
  type OrchestrationEvent,
  type OrchestrationSubscribeShellItem,
  type OrchestrationThreadSummary,
} from '@app/remote-chat-agent-contracts/orchestration';
import { create } from 'zustand';

import type { OrchestrationGateway } from '../data/orchestrationGateway';

export type CreateThreadInput = {
  title: string;
  cwd: string;
  modelSelection: ModelSelection;
  runtimeMode: RuntimeMode;
  interactionMode: InteractionMode;
  branch?: string;
  worktreePath?: string;
};

type ThreadShellState = {
  threads: OrchestrationThreadSummary[];
  loading: boolean;
  applyShellItem: (item: OrchestrationSubscribeShellItem) => void;
  refresh: (gateway: OrchestrationGateway) => Promise<void>;
  createThread: (
    gateway: OrchestrationGateway,
    input: CreateThreadInput,
  ) => Promise<{ receipt: OrchestrationCommandReceipt | null; threadId: string }>;
};

function compareByUpdatedAt(
  left: OrchestrationThreadSummary,
  right: OrchestrationThreadSummary,
): number {
  return (
    left.updatedAt.localeCompare(right.updatedAt) ||
    left.id.localeCompare(right.id)
  );
}

function projectEvent(
  threads: OrchestrationThreadSummary[],
  event: OrchestrationEvent,
): OrchestrationThreadSummary[] {
  switch (event.type) {
    case 'thread.created': {
      const payload = event.payload;
      const summary: OrchestrationThreadSummary = {
        id: event.threadId,
        title: payload.title,
        cwd: payload.cwd,
        modelSelection: payload.modelSelection,
        runtimeMode: payload.runtimeMode,
        interactionMode: payload.interactionMode,
        latestTurn: null,
        createdAt: payload.createdAt,
        updatedAt: payload.createdAt,
        archivedAt: null,
        deletedAt: null,
        session: null,
        ...(payload.branch !== undefined ? { branch: payload.branch } : {}),
        ...(payload.worktreePath !== undefined
          ? { worktreePath: payload.worktreePath }
          : {}),
      };
      const withoutDuplicates = threads.filter(
        (entry) => entry.id !== summary.id,
      );
      return [...withoutDuplicates, summary].sort(compareByUpdatedAt);
    }
    case 'thread.deleted': {
      return threads.map((entry) =>
        entry.id === event.threadId
          ? {
              ...entry,
              deletedAt: event.payload.deletedAt,
              updatedAt: event.payload.deletedAt,
            }
          : entry,
      );
    }
    case 'thread.archived': {
      return threads.map((entry) =>
        entry.id === event.threadId
          ? {
              ...entry,
              archivedAt: event.payload.archivedAt,
              updatedAt: event.payload.updatedAt,
            }
          : entry,
      );
    }
    case 'thread.unarchived': {
      return threads.map((entry) =>
        entry.id === event.threadId
          ? {
              ...entry,
              archivedAt: null,
              updatedAt: event.payload.updatedAt,
            }
          : entry,
      );
    }
    case 'thread.meta-updated': {
      return threads.map((entry) => {
        if (entry.id !== event.threadId) return entry;
        const payload = event.payload;
        return {
          ...entry,
          ...(payload.title !== undefined ? { title: payload.title } : {}),
          ...(payload.cwd !== undefined ? { cwd: payload.cwd } : {}),
          ...(payload.modelSelection !== undefined
            ? { modelSelection: payload.modelSelection }
            : {}),
          ...(payload.branch !== undefined ? { branch: payload.branch } : {}),
          ...(payload.worktreePath !== undefined
            ? { worktreePath: payload.worktreePath }
            : {}),
          updatedAt: payload.updatedAt,
        };
      });
    }
    case 'thread.runtime-mode-set': {
      return threads.map((entry) =>
        entry.id === event.threadId
          ? {
              ...entry,
              runtimeMode: event.payload.runtimeMode,
              updatedAt: event.payload.updatedAt,
            }
          : entry,
      );
    }
    case 'thread.interaction-mode-set': {
      return threads.map((entry) =>
        entry.id === event.threadId
          ? {
              ...entry,
              interactionMode: event.payload.interactionMode,
              updatedAt: event.payload.updatedAt,
            }
          : entry,
      );
    }
    case 'thread.turn-start-requested': {
      return threads.map((entry) =>
        entry.id === event.threadId
          ? {
              ...entry,
              ...(event.payload.modelSelection !== undefined
                ? { modelSelection: event.payload.modelSelection }
                : {}),
              runtimeMode: event.payload.runtimeMode,
              interactionMode: event.payload.interactionMode,
              updatedAt: event.createdAt,
            }
          : entry,
      );
    }
    default:
      return threads;
  }
}

function nextCommandId(): string {
  return `cmd-${Date.now().toString(36)}-${Math.random()
    .toString(36)
    .slice(2, 8)}`;
}

function nextThreadId(): string {
  return `th-${Date.now().toString(36)}-${Math.random()
    .toString(36)
    .slice(2, 8)}`;
}

export const useThreadShellStore = create<ThreadShellState>((set, get) => ({
  threads: [],
  loading: false,
  applyShellItem: (item) => {
    switch (item.kind) {
      case 'synchronized':
        Logger.debug('coding-agent.store.thread_shell.apply_shell_item', {
          kind: 'synchronized',
        });
        return;
      case 'snapshot': {
        const count = item.snapshot.length;
        Logger.info('coding-agent.store.thread_shell.apply_shell_item', {
          kind: 'snapshot',
          count,
        });
        set({ threads: [...item.snapshot].sort(compareByUpdatedAt) });
        return;
      }
      case 'event': {
        const threads = get().threads;
        const next = projectEvent(threads, item.event);
        if (next === threads) return;
        Logger.debug('coding-agent.store.thread_shell.apply_shell_item', {
          kind: 'event',
          type: item.event.type,
        });
        set({ threads: next });
        return;
      }
    }
  },
  refresh: async (gateway) => {
    Logger.info('coding-agent.store.thread_shell.refresh', {});
    set({ loading: true });
    try {
      const threads = await gateway.listThreads();
      Logger.info('coding-agent.store.thread_shell.refreshed', {
        count: threads.length,
      });
      set({ threads: [...threads].sort(compareByUpdatedAt), loading: false });
    } catch (cause) {
      Logger.withError(cause).warn(
        'coding-agent.store.thread_shell.refresh_failed',
      );
      set({ loading: false });
    }
  },
  createThread: async (gateway, input) => {
    const commandId = CommandId.parse(nextCommandId());
    const threadId = ThreadId.parse(nextThreadId());
    const createdAt = new Date().toISOString();
    Logger.info('coding-agent.store.thread_shell.create_thread', {
      threadId,
      titleLength: input.title.length,
    });
    try {
      const receipt = await gateway.dispatch({
        type: 'thread.create',
        commandId,
        threadId,
        title: input.title,
        cwd: input.cwd,
        modelSelection: input.modelSelection,
        runtimeMode: input.runtimeMode,
        interactionMode: input.interactionMode,
        createdAt,
        ...(input.branch !== undefined ? { branch: input.branch } : {}),
        ...(input.worktreePath !== undefined
          ? { worktreePath: input.worktreePath }
          : {}),
      });
      const done = {
        commandId,
        threadId,
        status: receipt.status,
        code: receipt.error?.code,
      };
      if (receipt.status === 'rejected') {
        Logger.warn('coding-agent.store.thread_shell.create_thread_done', done);
      } else {
        Logger.info('coding-agent.store.thread_shell.create_thread_done', done);
      }
      return { receipt, threadId };
    } catch (cause) {
      Logger.withError(cause).warn(
        'coding-agent.store.thread_shell.create_thread_failed',
      );
      return { receipt: null, threadId };
    }
  },
}));
