import { describe, expect, it } from 'vitest';

import type { Mark, MarkSpec } from '../../src';
import { clearMarks, mapMarks, setMarks } from '../../src/core/marks';

const mark = (id: string, from: number, to: number, hidden?: true): Mark => ({ id, kind: 'error', range: { from, to }, ...(hidden ? { hidden } : {}) });
const spec = (id: string, from: number, to: number, hidden?: boolean): MarkSpec => ({ id, kind: 'edit', range: { from, to }, ...(hidden === undefined ? {} : { hidden }) });
const ranges = (list: readonly Mark[]) => list.map(({ id, range }) => [id, range.from, range.to]);

describe('mapping marks through changes', () => {
  it('given an insert before the marks, when they are mapped, then each shifts by the inserted length', () => {
    const mapped = mapMarks([mark('a', 4, 6), mark('b', 8, 9)], [{ from: 1, to: 1, insert: 'xyz' }]);

    expect(ranges(mapped)).toEqual([['a', 7, 9], ['b', 11, 12]]);
  });

  it('given an insert after the marks, when they are mapped, then they stay', () => {
    expect(ranges(mapMarks([mark('a', 4, 6)], [{ from: 9, to: 9, insert: 'xyz' }]))).toEqual([['a', 4, 6]]);
  });

  it('given an insert exactly at the start of a mark and one exactly at its end, when mapped, then the inserted text stays outside', () => {
    expect(ranges(mapMarks([mark('a', 4, 6)], [{ from: 4, to: 4, insert: 'xy' }]))).toEqual([['a', 6, 8]]);
    expect(ranges(mapMarks([mark('a', 4, 6)], [{ from: 6, to: 6, insert: 'xy' }]))).toEqual([['a', 4, 6]]);
  });

  it('given an insert inside a mark, when mapped, then the mark grows by it', () => {
    expect(ranges(mapMarks([mark('a', 4, 8)], [{ from: 6, to: 6, insert: 'xy' }]))).toEqual([['a', 4, 10]]);
  });

  it('given a replace that covers the whole mark and more, when mapped, then the mark is dropped', () => {
    expect(mapMarks([mark('a', 5, 7), mark('b', 12, 14)], [{ from: 4, to: 8, insert: 'Q' }]).map(({ id }) => id)).toEqual(['b']);
  });

  it('given a delete of part of a mark, when mapped, then the rest stays marked', () => {
    expect(ranges(mapMarks([mark('a', 4, 8)], [{ from: 6, to: 9, insert: '' }]))).toEqual([['a', 4, 6]]);
  });

  it('given an empty mark and a replace around it, when mapped, then it is kept', () => {
    const mapped = mapMarks([mark('blank', 5, 5)], [{ from: 3, to: 8, insert: 'Q' }]);

    expect(mapped).toHaveLength(1);
    expect(mapped[0]?.range.from).toBe(mapped[0]?.range.to);
  });

  it('given several simultaneous changes, when mapped, then each position goes through all of them, and kind and hidden are kept', () => {
    const mapped = mapMarks([mark('a', 5, 7, true)], [{ from: 0, to: 2, insert: '' }, { from: 3, to: 3, insert: 'abc' }]);

    expect(mapped).toEqual([{ id: 'a', kind: 'error', range: { from: 6, to: 8 }, hidden: true }]);
  });
});

describe('setting and clearing marks', () => {
  it('given a spec with a new id and one with the id of a mark, when set, then the new one is added at the end and the other is replaced where it stands', () => {
    const next = setMarks([mark('a', 1, 2), mark('b', 3, 4)], [spec('a', 7, 8), spec('c', 5, 6)]);

    expect(ranges(next)).toEqual([['a', 7, 8], ['b', 3, 4], ['c', 5, 6]]);
  });

  it('given a spec that is hidden, when set, then the mark says hidden, and given one that is not, then the mark has no hidden field', () => {
    const [hidden, shown] = setMarks([], [spec('h', 1, 2, true), spec('s', 3, 4, false)]);

    expect(hidden).toEqual({ id: 'h', kind: 'edit', range: { from: 1, to: 2 }, hidden: true });
    expect(shown).toEqual({ id: 's', kind: 'edit', range: { from: 3, to: 4 } });
  });

  it('given marks, when some ids are cleared, then the others stay in order, and when all are cleared then none stay', () => {
    const list = [mark('a', 1, 2), mark('b', 3, 4), mark('c', 5, 6)];

    expect(clearMarks(list, ['b', 'zz']).map(({ id }) => id)).toEqual(['a', 'c']);
    expect(clearMarks(list, 'all')).toEqual([]);
  });

  it('given the list that was passed, when marks are set or cleared, then the list is not changed', () => {
    const list = [mark('a', 1, 2)];

    setMarks(list, [spec('a', 5, 6)]);
    clearMarks(list, 'all');

    expect(ranges(list)).toEqual([['a', 1, 2]]);
  });
});
