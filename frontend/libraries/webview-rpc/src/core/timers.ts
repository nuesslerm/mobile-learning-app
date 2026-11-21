export type Timers = {
  set(fn: () => void, ms: number): unknown;
  clear(handle: unknown): void;
};

const host = globalThis as unknown as { setTimeout(fn: () => void, ms: number): unknown; clearTimeout(handle: unknown): void };

export const realTimers: Timers = {
  set: (fn, ms) => host.setTimeout(fn, ms),
  clear: (handle) => host.clearTimeout(handle),
};
