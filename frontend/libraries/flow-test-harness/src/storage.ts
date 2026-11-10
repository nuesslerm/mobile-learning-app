// The async-storage stub keeps its map on `globalThis.__flowStorage`; tests
// seed or inspect persisted values through this handle.
const g = globalThis as { __flowStorage?: Map<string, string> };

function store(): Map<string, string> {
  g.__flowStorage ??= new Map();
  return g.__flowStorage;
}

export interface StorageHarness {
  seed(entries: Record<string, string>): void;
  get(key: string): string | undefined;
  keys(): string[];
  reset(): void;
}

export function storageHarness(): StorageHarness {
  return {
    seed: (entries) => {
      for (const [k, v] of Object.entries(entries)) store().set(k, v);
    },
    get: (key) => store().get(key),
    keys: () => [...store().keys()],
    reset: resetStorageHarness,
  };
}

export function resetStorageHarness(): void {
  store().clear();
}
