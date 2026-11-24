import { describe, expect, it } from 'vitest';

import { createLatestWins } from '../latestWins';

type Gate = { promise: Promise<void>; resolve: () => void; reject: (error: Error) => void };
const gate = (): Gate => {
  let resolve!: () => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<void>((ok, fail) => {
    resolve = ok;
    reject = fail;
  });
  return { promise, resolve, reject };
};

// a sender that records what it was given and holds the first reply behind a gate the test releases
const recorded = (first?: Gate) => {
  const sent: number[] = [];
  const send = (value: number) => {
    sent.push(value);
    return sent.length === 1 && first ? first.promise : Promise.resolve();
  };
  return { sent, send };
};

describe('createLatestWins', () => {
  it('given nothing in flight, when a value is pushed, then it is sent at once', () => {
    const { sent, send } = recorded();

    createLatestWins(send).push(1);

    expect(sent).toEqual([1]);
  });

  it('given a send held open, when twenty values are pushed, then exactly one more send follows the reply, carrying the newest, and the nineteen between are never sent', async () => {
    const first = gate();
    const { sent, send } = recorded(first);
    const latest = createLatestWins(send);

    for (let value = 1; value <= 20; value += 1) latest.push(value);
    expect(sent).toEqual([1]);
    first.resolve();
    await latest.settled();

    expect(sent).toEqual([1, 20]);
  });

  it('given the second send is held open too, when more values are pushed, then they wait for its reply and only the newest goes out', async () => {
    const first = gate();
    const second = gate();
    const secondSent = gate();
    const sent: number[] = [];
    const latest = createLatestWins((value: number) => {
      sent.push(value);
      if (sent.length === 2) secondSent.resolve();
      return sent.length === 1 ? first.promise : sent.length === 2 ? second.promise : Promise.resolve();
    });

    latest.push(1);
    latest.push(2);
    first.resolve();
    await secondSent.promise;
    latest.push(3);
    latest.push(4);
    expect(sent).toEqual([1, 2]);
    second.resolve();
    await latest.settled();

    expect(sent).toEqual([1, 2, 4]);
  });

  it('given a send that rejects, when another value is pushed, then it still goes out', async () => {
    const first = gate();
    const { sent, send } = recorded(first);
    const latest = createLatestWins(send);

    latest.push(1);
    first.reject(new Error('the editor went away'));
    await latest.settled();
    latest.push(2);

    expect(sent).toEqual([1, 2]);
  });

  it('given a send that rejects while a value waits, when the reply fails, then the waiting value goes out', async () => {
    const first = gate();
    const { sent, send } = recorded(first);
    const latest = createLatestWins(send);

    latest.push(1);
    latest.push(2);
    first.reject(new Error('the editor went away'));
    await latest.settled();

    expect(sent).toEqual([1, 2]);
  });

  it('given a send that throws before it returns, when another value is pushed, then the sender is not stuck', async () => {
    const sent: number[] = [];
    const latest = createLatestWins((value: number) => {
      sent.push(value);
      if (sent.length === 1) throw new Error('not ready');
      return Promise.resolve();
    });

    latest.push(1);
    await latest.settled();
    latest.push(2);

    expect(sent).toEqual([1, 2]);
  });

  it('given a value equal to the last one sent, when it is pushed again, then it is not sent again', async () => {
    const { sent, send } = recorded();
    const latest = createLatestWins(send);

    latest.push(7);
    await latest.settled();
    latest.push(7);

    expect(sent).toEqual([7]);
  });

  it('given a value equal to the one in flight or the one waiting, when it is pushed, then nothing more is queued', async () => {
    const first = gate();
    const { sent, send } = recorded(first);
    const latest = createLatestWins(send);

    latest.push(1);
    latest.push(1);
    latest.push(2);
    latest.push(2);
    first.resolve();
    await latest.settled();

    expect(sent).toEqual([1, 2]);
  });

  it('given the waiting value ends up equal to the one in flight, when the reply comes, then it is not sent again', async () => {
    const first = gate();
    const { sent, send } = recorded(first);
    const latest = createLatestWins(send);

    latest.push(1);
    latest.push(2);
    latest.push(1);
    first.resolve();
    await latest.settled();

    expect(sent).toEqual([1]);
  });

  it('given a value whose send was rejected, when the same value is pushed again, then it is sent again', async () => {
    const first = gate();
    const { sent, send } = recorded(first);
    const latest = createLatestWins(send);

    latest.push(5);
    first.reject(new Error('the editor went away'));
    await latest.settled();
    latest.push(5);

    expect(sent).toEqual([5, 5]);
  });

  it('given objects and an equality for them, when an equal one is pushed, then it is not sent again', async () => {
    const sent: { at: number }[] = [];
    const latest = createLatestWins(
      (value: { at: number }) => {
        sent.push(value);
        return Promise.resolve();
      },
      { equal: (a, b) => a.at === b.at }
    );

    latest.push({ at: 3 });
    await latest.settled();
    latest.push({ at: 3 });
    latest.push({ at: 4 });

    expect(sent).toEqual([{ at: 3 }, { at: 4 }]);
  });

  it('given a waiting value, when the sender is cancelled, then the waiting value is dropped and settled resolves after the one in flight', async () => {
    const first = gate();
    const { sent, send } = recorded(first);
    const latest = createLatestWins(send);
    const order: string[] = [];

    latest.push(1);
    latest.push(2);
    latest.cancel();
    const settled = latest.settled().then(() => order.push('settled'));
    order.push('released');
    first.resolve();
    await settled;

    expect(order).toEqual(['released', 'settled']);
    expect(sent).toEqual([1]);
  });

  it('given a cancelled sender, when a value is pushed afterwards, then it is sent', async () => {
    const first = gate();
    const { sent, send } = recorded(first);
    const latest = createLatestWins(send);

    latest.push(1);
    latest.cancel();
    first.resolve();
    await latest.settled();
    latest.push(2);

    expect(sent).toEqual([1, 2]);
  });

  it('given nothing in flight, when settled is asked, then it resolves at once', async () => {
    await expect(createLatestWins(recorded().send).settled()).resolves.toBeUndefined();
  });
});
