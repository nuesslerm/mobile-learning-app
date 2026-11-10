import { describe, expect, it } from 'vitest';

import {
  systemClock,
  createFrozenClock,
  createSteppingClock,
} from '../src/primitives/clock';

describe('systemClock', () => {
  it('returns a Date.now() between the reads before and after it', () => {
    const before = Date.now();
    const v = systemClock.now();
    const after = Date.now();
    expect(v).toBeGreaterThanOrEqual(before);
    expect(v).toBeLessThanOrEqual(after);
  });
});

describe('createFrozenClock', () => {
  it('always returns the same value', () => {
    const c = createFrozenClock(1_700_000_000_000);
    expect(c.now()).toBe(1_700_000_000_000);
    expect(c.now()).toBe(1_700_000_000_000);
    expect(c.now()).toBe(1_700_000_000_000);
  });

  it('returns the value passed at construction', () => {
    expect(createFrozenClock(0).now()).toBe(0);
    expect(createFrozenClock(42).now()).toBe(42);
    expect(createFrozenClock(-1).now()).toBe(-1);
  });
});

describe('createSteppingClock', () => {
  it('returns initial value first, then advances by step', () => {
    const c = createSteppingClock(1000, 100);
    expect(c.now()).toBe(1000);
    expect(c.now()).toBe(1100);
    expect(c.now()).toBe(1200);
  });

  it('honors custom step size', () => {
    const c = createSteppingClock(0, 50);
    expect(c.now()).toBe(0);
    expect(c.now()).toBe(50);
    expect(c.now()).toBe(100);
  });
});
