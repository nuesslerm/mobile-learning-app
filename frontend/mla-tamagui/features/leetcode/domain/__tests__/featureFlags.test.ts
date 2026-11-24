import { describe, expect, it } from 'vitest';

import { DEFAULT_LEETCODE_FLAGS, resolveLeetCodeFlags } from '../featureFlags';

describe('resolveLeetCodeFlags', () => {
  it('Given no env value and no override, When the flags resolve, Then aiEdit is off', () => {
    expect(resolveLeetCodeFlags({})).toEqual({ aiEdit: false, cursorPad: false });
    expect(DEFAULT_LEETCODE_FLAGS.aiEdit).toBe(false);
  });

  it('Given the env value 1, When the flags resolve, Then aiEdit is on', () => {
    expect(resolveLeetCodeFlags({ aiEdit: '1' }).aiEdit).toBe(true);
  });

  it('Given the env value 0, When the flags resolve, Then aiEdit is off', () => {
    expect(resolveLeetCodeFlags({ aiEdit: '0' }).aiEdit).toBe(false);
  });

  it('Given an env value that is neither 1 nor 0, When the flags resolve, Then the default holds', () => {
    expect(resolveLeetCodeFlags({ aiEdit: 'true' }).aiEdit).toBe(DEFAULT_LEETCODE_FLAGS.aiEdit);
    expect(resolveLeetCodeFlags({ aiEdit: '' }).aiEdit).toBe(DEFAULT_LEETCODE_FLAGS.aiEdit);
  });

  it('Given env 0 and an override of true, When the flags resolve, Then the override wins', () => {
    expect(resolveLeetCodeFlags({ aiEdit: '0' }, { aiEdit: true }).aiEdit).toBe(true);
  });

  it('Given env 1 and an override of false, When the flags resolve, Then the override wins', () => {
    expect(resolveLeetCodeFlags({ aiEdit: '1' }, { aiEdit: false }).aiEdit).toBe(false);
  });

  it('Given env 1 and an override that names no flag, When the flags resolve, Then the env value holds', () => {
    expect(resolveLeetCodeFlags({ aiEdit: '1' }, {}).aiEdit).toBe(true);
  });
});
