'use strict';
// The module object IS the storage API: `import AsyncStorage from '...'` may
// receive the whole module under CommonJS interop, so `default.getItem` must
// resolve either way. The map lives on globalThis for the storage harness.
const store = (globalThis.__flowStorage ??= new Map());
const AsyncStorage = {
  getItem: async (k) => (store.has(k) ? store.get(k) : null),
  setItem: async (k, v) => { store.set(k, String(v)); },
  removeItem: async (k) => { store.delete(k); },
  mergeItem: async (k, v) => { store.set(k, JSON.stringify({ ...JSON.parse(store.get(k) ?? '{}'), ...JSON.parse(v) })); },
  clear: async () => store.clear(),
  getAllKeys: async () => [...store.keys()],
  multiGet: async (ks) => ks.map((k) => [k, store.get(k) ?? null]),
  multiSet: async (kv) => { for (const [k, v] of kv) store.set(k, String(v)); },
  multiRemove: async (ks) => { for (const k of ks) store.delete(k); },
};
AsyncStorage.__esModule = true;
AsyncStorage.default = AsyncStorage;
AsyncStorage.useAsyncStorage = (k) => ({ getItem: () => AsyncStorage.getItem(k), setItem: (v) => AsyncStorage.setItem(k, v), removeItem: () => AsyncStorage.removeItem(k) });
module.exports = AsyncStorage;
