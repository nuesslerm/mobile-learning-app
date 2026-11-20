import { describe, expect, it } from 'vitest';

import { planBatch, type DocView, type ResolvedStep } from '../../src';

const doc = (text: string): DocView => ({ length: text.length, slice: (from, to) => text.slice(from, to) });
const replace = (from: number, to: number, text: string): ResolvedStep => ({ op: 'replace', range: { from, to }, text });
const move = (from: number, to: number, destination: number): ResolvedStep => ({ op: 'move', range: { from, to }, to: destination });

describe('planBatch', () => {
  it.each([
    ['a replace, an insert and a delete keep base coordinates', [replace(2, 4, 'XYZ'), replace(6, 6, '-'), replace(8, 9, '')], [{ from: 2, to: 4, insert: 'XYZ' }, { from: 6, to: 6, insert: '-' }, { from: 8, to: 9, insert: '' }]],
    ['equal-position inserts keep step order', [replace(3, 3, 'X'), replace(3, 3, 'Y')], [{ from: 3, to: 3, insert: 'X' }, { from: 3, to: 3, insert: 'Y' }]],
    ['an insert at the start of a replaced range goes before it', [replace(5, 7, 'ZZ'), replace(5, 5, 'I')], [{ from: 5, to: 5, insert: 'I' }, { from: 5, to: 7, insert: 'ZZ' }]],
    ['touching ranges are allowed', [replace(2, 4, 'a'), replace(4, 6, 'b')], [{ from: 2, to: 4, insert: 'a' }, { from: 4, to: 6, insert: 'b' }]],
    ['a move is a delete and an insert', [move(1, 3, 7)], [{ from: 1, to: 3, insert: '' }, { from: 7, to: 7, insert: '12' }]],
  ])('given %s, when planned, then the changes are exact', (_name, steps, expected) => {
    const planned = planBatch(doc('0123456789'), steps);
    expect(planned.ok && planned.plan.changes).toEqual(expected);
  });

  it.each([
    ['overlapping ranges', [replace(2, 6, 'a'), replace(4, 8, 'b')], 'overlap'],
    ['an insert inside a replaced range', [replace(2, 6, 'a'), replace(4, 4, 'b')], 'overlap'],
    ['a range past the end', [replace(5, 11, 'x')], 'out-of-range'],
    ['a move into itself', [move(3, 6, 4)], 'invalid'],
    ['two select steps', [{ op: 'select', anchor: 1 }, { op: 'select', anchor: 2 }], 'invalid'],
  ] as [string, ResolvedStep[], string][])('given %s, when planned, then it is refused as %s', (_name, steps, code) => {
    const planned = planBatch(doc('0123456789'), steps);
    expect(!planned.ok && planned.error.code).toBe(code);
  });

  it.each([
    ['moves to either boundary', [move(3, 5, 3)]],
    ['moves an empty range', [move(4, 4, 8)]],
    ['replaces nothing with nothing', [replace(4, 4, '')]],
  ] as [string, ResolvedStep[]][])('given a step that %s, when planned, then there are no changes', (_name, steps) => {
    const planned = planBatch(doc('0123456789'), steps);
    expect(planned.ok && planned.plan.changes).toEqual([]);
  });

  it('given two inserts then a move, when planned, then landed says where each inserted text sits in the result', () => {
    const planned = planBatch(doc('0123456789'), [replace(0, 0, 'ab'), move(4, 6, 9), replace(8, 8, 'Q')]);
    expect(planned.ok && planned.plan.landed).toEqual([{ from: 0, to: 2 }, { from: 10, to: 12 }, { from: 8, to: 9 }]);
  });
});
