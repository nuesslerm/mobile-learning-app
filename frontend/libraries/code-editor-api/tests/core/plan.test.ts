import { describe, expect, it } from 'vitest';

import { planBatch, type DocView, type ResolvedStep } from '../../src';

const doc = (text: string): DocView => ({ length: text.length, slice: (from, to) => text.slice(from, to) });
const replace = (from: number, to: number, text: string): ResolvedStep => ({ op: 'replace', range: { from, to }, text });
const move = (from: number, to: number, destination: number): ResolvedStep => ({ op: 'move', range: { from, to }, to: destination });
const splice = (edits: { from: number; to: number; insert: string }[], landed: { edit: number; from: number; to: number } | null = null): ResolvedStep => ({ op: 'splice', edits, landed });

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

describe('planBatch with a splice step', () => {
  const text = '0123456789';

  it('given a splice with a delete and an insert, when planned, then the changes are exact and landed names the inner range of the insert', () => {
    const edits = [{ from: 1, to: 3, insert: '' }, { from: 7, to: 7, insert: 'abcd' }];

    const whole = planBatch(doc(text), [splice(edits, { edit: 1, from: 0, to: 4 })]);
    const inner = planBatch(doc(text), [splice(edits, { edit: 1, from: 1, to: 3 })]);

    expect(whole.ok && whole.plan.changes).toEqual(edits);
    expect(whole.ok && whole.plan.landed).toEqual([{ from: 5, to: 9 }]);
    expect(inner.ok && inner.plan.landed).toEqual([{ from: 6, to: 8 }]);
  });

  it('given a splice that touches another step replace, when planned, then overlap names both step numbers', () => {
    const planned = planBatch(doc(text), [splice([{ from: 2, to: 4, insert: 'x' }]), replace(3, 5, 'y')]);

    expect(!planned.ok && planned.error).toMatchObject({ code: 'overlap', steps: [0, 1] });
  });

  it('given two edits of one splice that touch the same text, when planned, then overlap names that step twice', () => {
    const planned = planBatch(doc(text), [splice([{ from: 2, to: 5, insert: 'x' }, { from: 4, to: 6, insert: 'y' }])]);

    expect(!planned.ok && planned.error).toMatchObject({ code: 'overlap', steps: [0, 0] });
  });

  it('given a delete-only splice, when planned, then landed is a collapsed range where the text was, carried through the other changes', () => {
    const alone = planBatch(doc(text), [splice([{ from: 4, to: 6, insert: '' }])]);
    const beside = planBatch(doc(text), [replace(0, 0, 'ab'), splice([{ from: 4, to: 6, insert: '' }])]);

    expect(alone.ok && alone.plan.landed).toEqual([{ from: 4, to: 4 }]);
    expect(beside.ok && beside.plan.landed).toEqual([{ from: 0, to: 2 }, { from: 6, to: 6 }]);
  });

  it('given a splice beside a select with a point at the end of the step, when planned, then the caret lands at the end of the inserted text', () => {
    const planned = planBatch(doc(text), [splice([{ from: 3, to: 3, insert: 'XY' }], { edit: 0, from: 0, to: 2 }), { op: 'select', anchor: { step: 0, edge: 'end' } }]);

    expect(planned.ok && planned.plan.select).toEqual({ anchor: 5, head: 5 });
  });

  it('given any splice, when planned, then written says where all its inserted text sits in the result', () => {
    const planned = planBatch(doc(text), [splice([{ from: 1, to: 3, insert: '' }, { from: 5, to: 6, insert: 'AB' }, { from: 8, to: 8, insert: 'C' }], { edit: 1, from: 0, to: 2 })]);

    expect(planned.ok && planned.plan.written).toEqual([{ from: 3, to: 8 }]);
    expect(planned.ok && planned.plan.landed).toEqual([{ from: 3, to: 5 }]);
  });

  it('given steps that insert nothing, when planned, then their written is null', () => {
    const planned = planBatch(doc(text), [splice([{ from: 1, to: 3, insert: '' }]), replace(5, 6, ''), replace(7, 7, ''), { op: 'select', anchor: 0 }]);

    expect(planned.ok && planned.plan.written).toEqual([null, null, null, null]);
  });

  it('given a replace and a move, when planned, then written is the range of the text each inserts', () => {
    const planned = planBatch(doc(text), [replace(0, 0, 'ab'), move(4, 6, 9)]);

    expect(planned.ok && planned.plan.written).toEqual([{ from: 0, to: 2 }, { from: 9, to: 11 }]);
  });

  it('given an edit past the end, when planned, then it is out-of-range naming the step; given a landed range the edit does not hold, then it is internal', () => {
    const past = planBatch(doc(text), [replace(0, 0, 'a'), splice([{ from: 8, to: 11, insert: '' }])]);
    const lost = planBatch(doc(text), [splice([{ from: 2, to: 2, insert: 'ab' }], { edit: 0, from: 0, to: 3 })]);
    const nameless = planBatch(doc(text), [splice([{ from: 2, to: 2, insert: 'ab' }], { edit: 1, from: 0, to: 1 })]);

    expect(!past.ok && past.error).toMatchObject({ code: 'out-of-range', step: 1 });
    expect(!lost.ok && lost.error.code).toBe('internal');
    expect(!nameless.ok && nameless.error.code).toBe('internal');
  });

  it('given a splice with no edit, and one whose only edit changes nothing, when planned, then there are no changes, nothing landed for the first and a caret for the second', () => {
    const planned = planBatch(doc(text), [splice([]), splice([{ from: 4, to: 4, insert: '' }])]);

    expect(planned.ok && planned.plan).toMatchObject({ changes: [], landed: [null, { from: 4, to: 4 }], written: [null, null] });
  });
});
