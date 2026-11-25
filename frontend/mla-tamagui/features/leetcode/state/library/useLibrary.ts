import { Logger } from '@lib/logging';
import { useEffect, useSyncExternalStore } from 'react';

import { putInList, type LibrarySnapshot, type LibraryStoragePort, type SavedProblemList } from '../../domain/library/library';
import { useLeetCodePorts } from '../ports';

const empty: LibrarySnapshot = {
  lists: [{ id: 'favorite', name: 'Favorite', description: '', private: false, problemIds: [] }],
  lastOpened: null,
};

function createLibraryState(storage: LibraryStoragePort) {
  const listeners = new Set<() => void>();
  let state = empty;
  let load: Promise<void> | undefined;
  let writes = Promise.resolve(true);
  const publish = (snapshot: LibrarySnapshot) => {
    state = snapshot;
    for (const listener of listeners) listener();
  };
  const hydrate = () => {
    load ??= storage.read().then((snapshot) => {
      if (snapshot) publish(snapshot);
    }).catch((error: unknown) => {
      Logger.withError(error).error('leetcode.library.read_failed');
    });
    return load;
  };
  const update = (change: (previous: LibrarySnapshot) => LibrarySnapshot) => {
    writes = writes.then(async () => {
      await hydrate();
      const next = change(state);
      if (next === state) return true;
      await storage.write(next);
      publish(next);
      return true;
    }).catch((error: unknown) => {
      Logger.withError(error).error('leetcode.library.write_failed');
      return false;
    });
    return writes;
  };
  return {
    hydrate,
    snapshot: () => state,
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    openProblem: (id: string) => update((previous) => ({ ...previous, lastOpened: id })),
    toggleCollection: (name: string) => update((previous) => {
      const saved = previous.savedCollections ?? [];
      return { ...previous, savedCollections: saved.includes(name) ? saved.filter((item) => item !== name) : [...saved, name] };
    }),
    save: (listId: string, problemId: string) => update((previous) => ({
      ...previous, lists: previous.lists.map((list) => list.id === listId ? putInList(list, problemId) : list),
    })),
    favorite: (problemId: string) => update((previous) => ({
      ...previous,
      lists: previous.lists.map((list) => list.id !== 'favorite' ? list : list.problemIds.includes(problemId)
        ? { ...list, problemIds: list.problemIds.filter((id) => id !== problemId) }
        : putInList(list, problemId)),
    })),
    create: (values: Omit<SavedProblemList, 'id' | 'problemIds'>) => update((previous) => ({
      ...previous,
      lists: [...previous.lists, { ...values, id: `list-${Date.now()}-${previous.lists.length}`, problemIds: [] }],
    })),
  };
}

const stores = new WeakMap<LibraryStoragePort, ReturnType<typeof createLibraryState>>();

export function useLibrary() {
  const { libraryStorage } = useLeetCodePorts();
  let store = stores.get(libraryStorage);
  if (!store) {
    store = createLibraryState(libraryStorage);
    stores.set(libraryStorage, store);
  }
  useEffect(() => { void store.hydrate(); }, [store]);
  return { ...useSyncExternalStore(store.subscribe, store.snapshot, store.snapshot), actions: store };
}
