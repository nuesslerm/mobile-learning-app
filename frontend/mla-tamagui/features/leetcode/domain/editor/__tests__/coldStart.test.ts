import { describe, expect, it } from 'vitest';

import { createColdStart, nextEditorNth } from '../coldStart';

const fakeClock = () => {
  let now = 0;
  return { clock: () => now, at: (t: number) => void (now = t) };
};

describe('the editor cold start', () => {
  it('given marks at known instants, when the report is taken, then it holds the differences', () => {
    const t = fakeClock();
    const start = createColdStart({ clock: t.clock, nth: 1, htmlBytes: 100 });
    t.at(10);
    start.mark('mount');
    t.at(30);
    start.mark('loadStart');
    t.at(130);
    start.mark('ready');
    t.at(180);
    start.mark('hydrated');

    expect(start.take({ evalMs: 42 })).toEqual({ nth: 1, htmlBytes: 100, evalMs: 42, mountToReadyMs: 120, readyToHydratedMs: 50, totalMs: 170, loadStartToReadyMs: 100 });
  });

  it('given a mark that never came, when the report is taken, then there is none', () => {
    const t = fakeClock();
    const start = createColdStart({ clock: t.clock, nth: 1, htmlBytes: 1 });
    start.mark('mount');
    start.mark('hydrated');

    expect(start.take({})).toBeNull();
  });

  it('given no load start and no page time, when the report is taken, then those two fields are left out', () => {
    const t = fakeClock();
    const start = createColdStart({ clock: t.clock, nth: 2, htmlBytes: 1 });
    start.mark('mount');
    t.at(5);
    start.mark('ready');
    start.mark('hydrated');

    expect(Object.keys(start.take({}) ?? {}).sort()).toEqual(['htmlBytes', 'mountToReadyMs', 'nth', 'readyToHydratedMs', 'totalMs']);
  });

  it('given a mark set twice, when the report is taken, then the first instant counts, and a second take is empty', () => {
    const t = fakeClock();
    const start = createColdStart({ clock: t.clock, nth: 1, htmlBytes: 1 });
    start.mark('mount');
    t.at(10);
    start.mark('ready');
    t.at(20);
    start.mark('ready');
    start.mark('hydrated');

    expect(start.take({})?.mountToReadyMs).toBe(10);
    expect(start.take({})).toBeNull();
  });

  it('given editors opened one after another, when each takes its index, then they count up by one', () => {
    const first = nextEditorNth();

    expect([nextEditorNth(), nextEditorNth()]).toEqual([first + 1, first + 2]);
  });
});
