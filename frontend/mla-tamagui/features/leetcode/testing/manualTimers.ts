import type { Timers } from '@lib/webview-rpc';

export type ManualTimers = Timers & {
  /** the delays of the timers set and not yet fired or cleared */
  readonly pending: readonly number[];
  /** the delay of every timer ever set, in the order they were set */
  readonly armed: readonly number[];
  /** Fires every pending timer given exactly `ms`, in the order they were set; answers how many it fired. */
  fire(ms: number): number;
};

/** The double of the Timers seam: nothing fires until the test says so, so a test waits on a signal and never on a clock. */
export function createManualTimers(): ManualTimers {
  let next = 1;
  const timers = new Map<number, { ms: number; fn: () => void }>();
  const armed: number[] = [];
  return {
    get pending() {
      return [...timers.values()].map((timer) => timer.ms);
    },
    get armed() {
      return armed;
    },
    set(fn, ms) {
      const id = next++;
      timers.set(id, { ms, fn });
      armed.push(ms);
      return id;
    },
    clear(handle) {
      timers.delete(handle as number);
    },
    fire(ms) {
      const due = [...timers.entries()].filter(([, timer]) => timer.ms === ms);
      for (const [id, timer] of due) {
        timers.delete(id);
        timer.fn();
      }
      return due.length;
    },
  };
}
