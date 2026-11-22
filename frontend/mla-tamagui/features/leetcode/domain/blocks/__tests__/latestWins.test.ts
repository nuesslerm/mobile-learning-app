import { describe, expect, it, vi } from 'vitest';

import { createRefresher } from '../latestWins';

/** Reads that stay open until the test resolves them, in the order they were started. */
function controlled() {
  const open: ((value: number) => void)[] = [];
  const rejects: ((error: unknown) => void)[] = [];
  const read = vi.fn(
    () =>
      new Promise<number>((resolve, reject) => {
        open.push(resolve);
        rejects.push(reject);
      })
  );
  return { read, open, rejects };
}

describe('createRefresher', () => {
  it('given nothing in flight, when a refresh is requested, then one read starts at once and its result is delivered', async () => {
    const { read, open } = controlled();
    const results: number[] = [];
    const refresher = createRefresher(read, (value) => results.push(value));

    refresher.request();
    expect(read).toHaveBeenCalledTimes(1);
    open[0]?.(1);
    await refresher.settled();

    expect(results).toEqual([1]);
  });

  it('given a read in flight, when two more refreshes are requested, then exactly one further read follows and only its result is delivered', async () => {
    const { read, open } = controlled();
    const results: number[] = [];
    const refresher = createRefresher(read, (value) => results.push(value));

    refresher.request();
    refresher.request();
    refresher.request();
    expect(read).toHaveBeenCalledTimes(1);
    open[0]?.(1);
    await vi.waitFor(() => expect(read).toHaveBeenCalledTimes(2));
    expect(results).toEqual([]);
    open[1]?.(2);
    await refresher.settled();

    expect(read).toHaveBeenCalledTimes(2);
    expect(results).toEqual([2]);
  });

  it('given a read in flight, when settled is asked, then it resolves only when the read and the one that followed it have ended', async () => {
    const { read, open } = controlled();
    const refresher = createRefresher(read, () => undefined);
    let settled = false;

    refresher.request();
    refresher.request();
    void refresher.settled().then(() => {
      settled = true;
    });
    open[0]?.(1);
    await vi.waitFor(() => expect(read).toHaveBeenCalledTimes(2));
    expect(settled).toBe(false);
    open[1]?.(2);
    await refresher.settled();

    expect(settled).toBe(true);
  });

  it('given a read in flight, when the refresher is stopped, then its late result is dropped and nothing is read again', async () => {
    const { read, open } = controlled();
    const results: number[] = [];
    const refresher = createRefresher(read, (value) => results.push(value));

    refresher.request();
    refresher.request();
    refresher.stop();
    open[0]?.(1);
    await refresher.settled();
    refresher.request();

    expect(results).toEqual([]);
    expect(read).toHaveBeenCalledTimes(1);
  });

  it('given a read that fails, when it ends, then nothing is delivered, the refresher is idle and the next request reads again', async () => {
    const { read, open, rejects } = controlled();
    const results: number[] = [];
    const refresher = createRefresher(read, (value) => results.push(value));

    refresher.request();
    rejects[0]?.(new Error('the page went away'));
    await refresher.settled();
    refresher.request();
    open[1]?.(5);
    await refresher.settled();

    expect(read).toHaveBeenCalledTimes(2);
    expect(results).toEqual([5]);
  });

  it('given nothing in flight, when settled is asked, then it resolves at once', async () => {
    const refresher = createRefresher(controlled().read, () => undefined);

    await expect(refresher.settled()).resolves.toBeUndefined();
  });
});
