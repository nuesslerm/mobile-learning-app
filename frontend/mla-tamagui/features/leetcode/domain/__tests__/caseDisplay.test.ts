import { describe, expect, it } from 'vitest';

import { clip, orderCases, showInput, showValue } from '../caseDisplay';
import type { TestResult } from '../execution';

const row = (pass: boolean, over: Partial<TestResult> = {}): TestResult => ({ pass, actual: 1, expected: 1, stdout: '', stderr: '', error: null, elapsed_ms: 1, ...over });

describe('orderCases', () => {
  it('Given passing and failing cases mixed, When ordered, Then the failures come first and each group keeps the bundled order', () => {
    const ordered = orderCases([row(true), row(false), row(true), row(false)]);
    expect(ordered.map((r) => [r.index, r.result.pass])).toEqual([[1, false], [3, false], [0, true], [2, true]]);
  });
});

describe('showValue and showInput', () => {
  it.each([
    ['a string', 'abc', 'abc'],
    ['a list', [1, [2, 3]], '[1,[2,3]]'],
    ['nothing', undefined, 'None'],
    ['null', null, 'null'],
    ['a boolean', true, 'true'],
  ])('Given %s, When shown, Then it is one line', (_name, value, expected) => {
    expect(showValue(value)).toBe(expected);
  });

  it('Given the arguments of a case, When shown, Then each is name = value in order on one line', () => {
    expect(showInput({ nums: [2, 7], target: 9 })).toBe('nums = [2,7], target = 9');
    expect(showInput(undefined)).toBe('');
  });
});

describe('clip', () => {
  it('Given a short value, When clipped, Then it is whole and not clipped', () => {
    expect(clip('short')).toEqual({ text: 'short', clipped: false });
  });

  it('Given a long value, When clipped, Then one line ends in an ellipsis and says it was clipped', () => {
    const { text, clipped } = clip('x'.repeat(100));
    expect(clipped).toBe(true);
    expect(text).toBe(`${'x'.repeat(72)}…`);
  });

  it('Given several lines, When clipped, Then only the first shows and it is clipped', () => {
    expect(clip('one\ntwo')).toEqual({ text: 'one…', clipped: true });
  });

  it.each([
    ['a printed line with its newline', 'x\n', { text: 'x', clipped: false }],
    ['a printed line with a Windows newline', 'x\r\n', { text: 'x', clipped: false }],
    ['text that starts with a blank line', '\nfoo', { text: 'foo…', clipped: true }],
    ['nothing but newlines', '\n\n', { text: '', clipped: false }],
  ])('Given %s, When clipped, Then the trailing newline is not a second line', (_name, value, expected) => {
    expect(clip(value)).toEqual(expected);
  });

  it('Given a very long run of whitespace before text, When clipped, Then it is quick (a backtracking trim was quadratic)', () => {
    const started = Date.now();
    clip(`${' '.repeat(200_000)}x`);
    expect(Date.now() - started).toBeLessThan(500);
  });

  it('Given an emoji that straddles the limit, When clipped, Then it is kept whole or left out, never cut in half', () => {
    const { text } = clip(`${'a'.repeat(71)}😀 and more`);
    expect(text).toBe(`${'a'.repeat(71)}😀…`);
    expect(clip(`${'a'.repeat(72)}😀`).text).toBe(`${'a'.repeat(72)}…`);
  });
});
