// Mockable time source. The package uses this for timestamps instead of
// calling `Date.now()` directly so tests can inject a frozen clock and
// the host can override (e.g. to use a virtual time from a custom
// scheduler). Production code uses `systemClock`; tests use
// `createFrozenClock(at)`.
//
//   const clock = createFrozenClock(1_700_000_000_000);
//   clock.now(); // → 1_700_000_000_000, every call, no real-time drift

export interface Clock {
  now(): number;
}

/** Real-time clock. Production code uses this. */
export const systemClock: Clock = {
  now: () => Date.now(),
};

/** Frozen clock that always returns the same value. For tests. */
export function createFrozenClock(at: number): Clock {
  return {
    now: () => at,
  };
}

/** Clock that advances by a fixed delta each call. For tests that
 *  need predictable intervals (e.g. `vi.advanceTimersByTime` doesn't
 *  propagate to `Date.now()` in some setups). */
export function createSteppingClock(initial: number, stepMs: number): Clock {
  let t = initial;
  return {
    now: () => {
      const v = t;
      t += stepMs;
      return v;
    },
  };
}
