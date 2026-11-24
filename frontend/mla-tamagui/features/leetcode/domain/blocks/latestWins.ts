import { Logger } from '@lib/logging';

export type Refresher = {
  /** reads now when nothing is in flight; otherwise marks the read in flight as out of date and reads once more when it ends */
  request(): void;
  /** resolves when nothing is in flight and nothing waits */
  settled(): Promise<void>;
  /** no result is delivered from now on, and nothing is read */
  stop(): void;
};

/**
 * One read in flight at most. A request made while it runs makes its result out of date: it is dropped, and one more read follows, however many requests came. The next read is released by the end of the
 * last one, not by a clock, so a burst of changes costs one extra read and the view only ever draws the newest one.
 */
export function createRefresher<T>(read: () => Promise<T>, onResult: (result: T) => void): Refresher {
  let inFlight = false;
  let dirty = false;
  let stopped = false;
  let idle: (() => void)[] = [];

  const release = () => {
    const waiting = idle;
    idle = [];
    for (const resolve of waiting) resolve();
  };

  const run = () => {
    inFlight = true;
    dirty = false;
    // the executor runs now, and a read that throws before it returns becomes a rejection
    void new Promise<T>((resolve) => resolve(read()))
      .then(
        (result) => {
          if (!stopped && !dirty) onResult(result);
        },
        (error: unknown) => Logger.withError(error).warn('leetcode.blocks.read_failed')
      )
      .then(() => {
        inFlight = false;
        if (dirty && !stopped) run();
        else release();
      });
  };

  return {
    request() {
      if (stopped) return;
      if (inFlight) dirty = true;
      else run();
    },
    settled: () => (inFlight ? new Promise<void>((resolve) => idle.push(resolve)) : Promise.resolve()),
    stop() {
      stopped = true;
      dirty = false;
      if (!inFlight) release();
    },
  };
}
