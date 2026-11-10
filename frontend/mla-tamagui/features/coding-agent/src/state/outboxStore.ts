// features/coding-agent/src/state/outboxStore.ts
//
// Zustand store for the offline-first command queue. The data shape and
// the pure ops (`enqueue`/`remove`/`markAttempt`/`nextDue`) live in
// `src/domain/outbox.ts` so they can be unit tested
// without IO; this module wraps them, holds the cached list, and
// re-persists after every mutation through `outboxStorage`. Persistence
// is fire-and-forget after a set: the in-memory cache is the source of
// truth for the drain (in `useOutboxDrain`) and the AsyncStorage write
// is a durability backstop — a crash between two writes can drop at
// most the last command, which `nextDue` would re-issue on its next
// retry window anyway. Failures log `coding-agent.outbox.persist_failed`
// rather than throwing, so a corrupt storage row doesn't take down the
// `enqueue` call site.
import { Logger } from '@lib/logging';
import {
  enqueue as enqueuePure,
  markAttempt as markAttemptPure,
  remove as removePure,
  type OutboxItem,
} from '../domain';
import { create } from 'zustand';

import {
  loadOutbox,
  saveOutbox,
  type OutboxStorageOptions,
} from '../data/outboxStorage';

type OutboxStoreState = {
  items: OutboxItem[];
  hydrated: boolean;
  enqueue: (item: OutboxItem, options?: OutboxStorageOptions) => void;
  remove: (id: string, options?: OutboxStorageOptions) => void;
  markAttempt: (id: string, options?: OutboxStorageOptions) => void;
  hydrate: (options?: OutboxStorageOptions) => Promise<void>;
  persist: (options?: OutboxStorageOptions) => Promise<void>;
};

export const useOutboxStore = create<OutboxStoreState>((set, get) => ({
  items: [],
  hydrated: false,
  enqueue: (item, options) => {
    set((state) => ({ items: enqueuePure(state.items, item) }));
    Logger.info('coding-agent.store.outbox.enqueue', {
      id: item.id,
      threadId: item.threadId,
    });
    void get().persist(options);
  },
  remove: (id, options) => {
    let removed = false;
    set((state) => {
      if (!state.items.some((entry) => entry.id === id)) return state;
      removed = true;
      return { items: removePure(state.items, id) };
    });
    if (removed) {
      Logger.info('coding-agent.store.outbox.remove', { id });
      void get().persist(options);
    }
  },
  markAttempt: (id, options) => {
    set((state) => ({ items: markAttemptPure(state.items, id) }));
    Logger.debug('coding-agent.store.outbox.mark_attempt', { id });
    void get().persist(options);
  },
  hydrate: async (options) => {
    const items = await loadOutbox(options ?? {});
    set({ items, hydrated: true });
    Logger.info('coding-agent.store.outbox.hydrate', { count: items.length });
  },
  persist: async (options) => {
    try {
      await saveOutbox(get().items, options ?? {});
    } catch (cause) {
      Logger.withError(cause).warn(
        'coding-agent.store.outbox.persist_failed',
      );
    }
  },
}));