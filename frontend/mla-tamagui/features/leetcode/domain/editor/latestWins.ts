import { Logger } from '@lib/logging';

/**
 * A latest-wins sender: at most one send is in flight, and while it is, only the newest pushed value waits. The next send is
 * released by the reply, not by a clock, so nothing queues and a fast producer costs one pending value however many it pushes.
 */

export type LatestWins<T> = {
  /** sends at once when nothing is in flight; otherwise keeps `value` as the one that goes next, replacing an older waiting one */
  push(value: T): void;
  /** resolves when nothing is in flight and nothing waits */
  settled(): Promise<void>;
  /** drops the waiting value; the one in flight still lands */
  cancel(): void;
};

export type LatestWinsOptions<T> = {
  /** a value equal to the last one sent (or the one waiting) is not sent again; by identity unless given */
  readonly equal?: (a: T, b: T) => boolean;
};

type Slot<T> = { readonly value: T };

export function createLatestWins<T>(send: (value: T) => Promise<unknown>, options: LatestWinsOptions<T> = {}): LatestWins<T> {
  const equal = options.equal ?? Object.is;
  let inFlight = false;
  let last: Slot<T> | null = null;
  let waiting: Slot<T> | null = null;
  let idle: (() => void)[] = [];

  const release = () => {
    inFlight = false;
    const next = waiting;
    waiting = null;
    if (next && !(last && equal(next.value, last.value))) {
      dispatch(next);
      return;
    }
    const done = idle;
    idle = [];
    for (const resolve of done) resolve();
  };

  const dispatch = (slot: Slot<T>) => {
    inFlight = true;
    last = slot;
    // the executor runs now, and a send that throws before it returns becomes a rejection
    new Promise<unknown>((resolve) => resolve(send(slot.value))).then(
      () => release(),
      (error: unknown) => {
        Logger.withError(error).warn('editor.latest_wins.send_failed');
        if (last === slot) last = null;
        release();
      }
    );
  };

  return {
    push(value) {
      const against = waiting ?? last;
      if (against && equal(value, against.value)) return;
      if (inFlight) {
        waiting = { value };
        return;
      }
      dispatch({ value });
    },
    settled: () => (inFlight ? new Promise<void>((resolve) => idle.push(resolve)) : Promise.resolve()),
    cancel() {
      waiting = null;
    },
  };
}
