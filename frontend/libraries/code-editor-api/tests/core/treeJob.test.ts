import { describe, expect, it } from 'vitest';

import { runTreeJob, type TreeJobClock } from '../../src/core/treeJob';
import type { TreeSnapshot } from '../../src/ports/host';

// a parser that needs `cost` ms of work in all and reports how it was driven
function source(cost: number) {
  let done = 0;
  const calls: number[] = [];
  const snapshot: TreeSnapshot = {
    language: 'python',
    length: 100,
    work(upto, ms) {
      calls.push(ms);
      done = Math.min(upto, done + ms);
      return done >= upto;
    },
    parsedTo: () => done,
    read: () => null,
  };
  return { snapshot, calls, upto: cost };
}

function clockOf(headAfterYield?: (yields: number) => number) {
  let time = 0;
  let head = 0;
  let yields = 0;
  const clock: TreeJobClock = {
    sliceMs: 8,
    now: () => time,
    head: () => head,
    yieldToEvents: async () => {
      yields += 1;
      if (headAfterYield) head = headAfterYield(yields);
    },
  };
  return { clock, advance: (ms: number) => (time += ms), yields: () => yields };
}

describe('runTreeJob', () => {
  it('given 40 ms of parse work and a 50 ms budget, when the job runs, then work comes in slices of at most 8 ms with a turn between each and the tree is complete', async () => {
    // Given a parser that needs 40 units and a clock that advances by the slice each time work is asked for
    const s = source(40);
    const c = clockOf();
    const parser: TreeSnapshot = { ...s.snapshot, work: (upto, ms) => { c.advance(ms); return s.snapshot.work(upto, ms); } };

    // When the job runs with a budget of 50 ms
    const result = await runTreeJob(parser, s.upto, 50, c.clock);

    // Then it was complete after five slices of 8 ms, with a turn given before each slice but the first
    expect(result).toEqual({ complete: true, parsedTo: 40 });
    expect(s.calls).toEqual([8, 8, 8, 8, 8]);
    expect(c.yields()).toBe(4);
  });

  it('given 100 ms of work and a 20 ms budget, when the job runs, then it stops incomplete at what it reached, within the budget and a slice', async () => {
    const s = source(100);
    const c = clockOf();
    const parser: TreeSnapshot = { ...s.snapshot, work: (upto, ms) => { c.advance(ms); return s.snapshot.work(upto, ms); } };

    const result = await runTreeJob(parser, s.upto, 20, c.clock);

    expect(result.complete).toBe(false);
    expect(result.parsedTo).toBe(20);
    expect(s.calls.reduce((a, b) => a + b, 0)).toBeLessThanOrEqual(20 + 8);
  });

  it('given a budget of zero, when the job runs, then the parser is probed once for no time and the job never gives a turn', async () => {
    const s = source(100);
    const c = clockOf();

    const result = await runTreeJob(s.snapshot, s.upto, 0, c.clock);

    expect(s.calls).toEqual([0]);
    expect(c.yields()).toBe(0);
    expect(result.complete).toBe(false);
  });

  it('given a document already parsed, when the job runs, then it is complete after the first slice and gives no turn', async () => {
    const s = source(0);
    const c = clockOf();

    const result = await runTreeJob(s.snapshot, 0, 50, c.clock);

    expect(result).toEqual({ complete: true, parsedTo: 0 });
    expect(c.yields()).toBe(0);
  });

  it('given the head moves while the job waits for a turn, when it resumes, then it stops incomplete without another slice', async () => {
    const s = source(100);
    const c = clockOf((yields) => yields);
    const parser: TreeSnapshot = { ...s.snapshot, work: (upto, ms) => { c.advance(ms); return s.snapshot.work(upto, ms); } };

    const result = await runTreeJob(parser, s.upto, 50, c.clock);

    expect(result).toEqual({ complete: false, parsedTo: 8 });
    expect(s.calls).toEqual([8]);
  });
});
