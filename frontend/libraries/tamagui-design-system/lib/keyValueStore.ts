/**
 * Dependency-free Zustand `StateStorage` adapter.
 *
 * - Web: `window.localStorage`.
 * - Anywhere else (native runtimes, node unit tests): an in-memory map.
 *
 * Zustand's `persist` middleware expects `getItem`/`setItem`/`removeItem`
 * to return `string | null` via `createJSONStorage`; this shim is the
 * `StateStorage` value passed to `createJSONStorage(() => keyValueStorage)`.
 */
const memoryStorage = new Map<string, string>();

function getWebStorage(): Storage | null {
  return typeof localStorage === 'undefined' ? null : localStorage;
}

export const keyValueStorage = {
  getItem: async (name: string): Promise<string | null> => {
    const webStorage = getWebStorage();
    if (webStorage) return webStorage.getItem(name);
    return memoryStorage.get(name) ?? null;
  },
  setItem: async (name: string, value: string): Promise<void> => {
    const webStorage = getWebStorage();
    if (webStorage) {
      webStorage.setItem(name, value);
      return;
    }
    memoryStorage.set(name, value);
  },
  removeItem: async (name: string): Promise<void> => {
    const webStorage = getWebStorage();
    if (webStorage) {
      webStorage.removeItem(name);
      return;
    }
    memoryStorage.delete(name);
  },
};
