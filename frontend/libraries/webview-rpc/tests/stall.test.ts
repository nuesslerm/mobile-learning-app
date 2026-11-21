import { describe, expect, it } from 'vitest';

import { createRpcClient, createRpcServer, type Timers } from '../src';
import { createLoopbackPair } from '../src/testing';

const settled = () => new Promise((resolve) => setTimeout(resolve, 5));

/** Timers and a clock the test moves by hand, so a call's expiry is exactly when the test says. */
function handTimers() {
  const waiting: { fn: () => void }[] = [];
  let at = 0;
  const timers: Timers = {
    set: (fn) => {
      const handle = { fn };
      waiting.push(handle);
      return handle;
    },
    clear: (handle) => {
      const index = waiting.indexOf(handle as { fn: () => void });
      if (index >= 0) waiting.splice(index, 1);
    },
  };
  return {
    timers,
    now: () => at,
    advance: (ms: number) => void (at += ms),
    fire: () => waiting.shift()?.fn(),
    waiting: () => waiting.length,
  };
}

function pageThatAnswersWhenTold() {
  const { host, page } = createLoopbackPair();
  let answer: (value: number) => void = () => undefined;
  createRpcServer(page, { slow: () => new Promise<number>((resolve) => (answer = resolve)) });
  return { host, answer: (value: number) => answer(value) };
}

describe('the rpc client timeout', () => {
  it('given a timer that fires long after its time because the thread was away, when the call has no answer yet, then it waits its time once more and the answer that comes then resolves it', async () => {
    // Given a call of 1000 ms whose timer the thread could not run until 1600 ms had passed
    const { host, answer } = pageThatAnswersWhenTold();
    const clock = handTimers();
    const client = createRpcClient(host, { timeoutMs: 1000, timers: clock.timers, now: clock.now });
    const call = client.call<number>('slow');
    await settled();

    // When the late timer fires, and the answer then arrives
    clock.advance(1600);
    clock.fire();
    expect(clock.waiting()).toBe(1);
    answer(7);

    // Then the call resolved instead of failing
    await expect(call).resolves.toBe(7);
  });

  it('given a timer that fires on time, when the call has no answer, then it fails with a timeout', async () => {
    // Given a call of 1000 ms and a page that never answers
    const { host } = pageThatAnswersWhenTold();
    const clock = handTimers();
    const client = createRpcClient(host, { timeoutMs: 1000, timers: clock.timers, now: clock.now });
    const call = client.call<number>('slow');
    await settled();

    // When its timer fires at 1000 ms
    clock.advance(1000);
    clock.fire();

    // Then it failed as a timeout
    await expect(call).rejects.toMatchObject({ code: 'timeout' });
  });

  it('given a timer that is late twice, when the call still has no answer, then it fails with a timeout: the extra wait is given once', async () => {
    // Given a call whose first timer fired late and was given its time once more
    const { host } = pageThatAnswersWhenTold();
    const clock = handTimers();
    const client = createRpcClient(host, { timeoutMs: 1000, timers: clock.timers, now: clock.now });
    const call = client.call<number>('slow');
    await settled();
    clock.advance(1600);
    clock.fire();

    // When the second timer is late too
    clock.advance(1600);
    clock.fire();

    // Then it failed as a timeout
    await expect(call).rejects.toMatchObject({ code: 'timeout' });
  });
});
