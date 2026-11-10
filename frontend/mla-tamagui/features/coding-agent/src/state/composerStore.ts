// features/coding-agent/src/state/composerStore.ts
//
// Zustand store for the thread composer's UI-only state: the in-progress
// draft text, the per-thread `runtimeMode` / `interactionMode` /
// `modelSelection` tri-state the chip row renders, and the `queued`
// backlog of text the user typed while a turn was running (drained when
// the next turn starts; dropped on `clearDraft` or thread teardown).
// These are pure UI affordances — the wire-bound values are mirrored to
// the thread via `useThreadCommands`, so changes here only matter until
// the next snapshot comes through. Per-thread scoping is what lets two
// open threads keep independent drafts without the typeahead bouncing
// between them when one re-renders.
import { Logger } from '@lib/logging';
import type {
  InteractionMode,
  ModelSelection,
  RuntimeMode,
} from '../domain';
import type { ThreadId } from '@app/remote-chat-agent-contracts/orchestration';
import { create } from 'zustand';

const DEFAULT_RUNTIME_MODE: RuntimeMode = 'approval-required';
const DEFAULT_INTERACTION_MODE: InteractionMode = 'default';

export type QueuedThreadMessage = string;
export const EMPTY_QUEUE: readonly QueuedThreadMessage[] = [];

type ComposerStoreState = {
  drafts: Record<ThreadId, string>;
  runtimeMode: Record<ThreadId, RuntimeMode>;
  interactionMode: Record<ThreadId, InteractionMode>;
  modelSelection: Record<ThreadId, ModelSelection | null>;
  queued: Record<ThreadId, QueuedThreadMessage[]>;
  setDraft: (threadId: ThreadId, text: string) => void;
  clearDraft: (threadId: ThreadId) => void;
  setRuntimeMode: (threadId: ThreadId, mode: RuntimeMode) => void;
  setInteractionMode: (threadId: ThreadId, mode: InteractionMode) => void;
  setModelSelection: (
    threadId: ThreadId,
    selection: ModelSelection | null,
  ) => void;
  enqueueQueued: (threadId: ThreadId, text: string) => void;
  dequeueQueued: (threadId: ThreadId) => string | undefined;
  discardThread: (threadId: ThreadId) => void;
};

export const useComposerStore = create<ComposerStoreState>((set, get) => ({
  drafts: {},
  runtimeMode: {},
  interactionMode: {},
  modelSelection: {},
  queued: {},
  setDraft: (threadId, text) => {
    set((state) => {
      if (state.drafts[threadId] === text) return state;
      return { drafts: { ...state.drafts, [threadId]: text } };
    });
  },
  clearDraft: (threadId) => {
    set((state) => {
      if (
        state.drafts[threadId] === undefined &&
        state.queued[threadId] === undefined
      ) {
        return state;
      }
      const drafts = { ...state.drafts };
      const queued = { ...state.queued };
      delete drafts[threadId];
      delete queued[threadId];
      return { drafts, queued };
    });
  },
  setRuntimeMode: (threadId, mode) => {
    set((state) => {
      if (state.runtimeMode[threadId] === mode) return state;
      Logger.info('coding-agent.store.composer.set_runtime_mode', {
        threadId,
        mode,
      });
      return { runtimeMode: { ...state.runtimeMode, [threadId]: mode } };
    });
  },
  setInteractionMode: (threadId, mode) => {
    set((state) => {
      if (state.interactionMode[threadId] === mode) return state;
      Logger.info('coding-agent.store.composer.set_interaction_mode', {
        threadId,
        mode,
      });
      return { interactionMode: { ...state.interactionMode, [threadId]: mode } };
    });
  },
  setModelSelection: (threadId, selection) => {
    set((state) => {
      if (state.modelSelection[threadId] === selection) return state;
      Logger.info('coding-agent.store.composer.set_model_selection', {
        threadId,
        cli: selection?.cli ?? null,
      });
      return { modelSelection: { ...state.modelSelection, [threadId]: selection } };
    });
  },
  enqueueQueued: (threadId, text) => {
    if (text.length === 0) return;
    set((state) => {
      const previous = state.queued[threadId] ?? [];
      return {
        queued: { ...state.queued, [threadId]: [...previous, text] },
      };
    });
  },
  dequeueQueued: (threadId) => {
    const queue = get().queued[threadId];
    if (queue === undefined || queue.length === 0) return undefined;
    const [head, ...rest] = queue;
    set((state) => ({
      queued: { ...state.queued, [threadId]: rest },
    }));
    return head;
  },
  discardThread: (threadId) => {
    set((state) => {
      const drafts = { ...state.drafts };
      const runtimeMode = { ...state.runtimeMode };
      const interactionMode = { ...state.interactionMode };
      const modelSelection = { ...state.modelSelection };
      const queued = { ...state.queued };
      delete drafts[threadId];
      delete runtimeMode[threadId];
      delete interactionMode[threadId];
      delete modelSelection[threadId];
      delete queued[threadId];
      return { drafts, runtimeMode, interactionMode, modelSelection, queued };
    });
  },
}));

export function getDefaultRuntimeMode(threadId: ThreadId): RuntimeMode {
  return useComposerStore.getState().runtimeMode[threadId] ?? DEFAULT_RUNTIME_MODE;
}

export function getDefaultInteractionMode(
  threadId: ThreadId,
): InteractionMode {
  return (
    useComposerStore.getState().interactionMode[threadId] ?? DEFAULT_INTERACTION_MODE
  );
}