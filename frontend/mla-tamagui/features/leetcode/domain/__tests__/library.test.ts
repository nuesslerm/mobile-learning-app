import { describe, expect, it } from 'vitest';

import { filterLibrary, putInList, type LibraryRow, type SavedProblemList } from '../library/library';

describe('Algorithms library', () => {
  it('Given an Algorithms collection, When search contains multiple words, Then matching titles retain their order', () => {
    const rows: LibraryRow[] = [
      { id: 'two-sum', number: 1, title: 'Two Sum', difficulty: 'easy' },
      { id: 'add-two', number: 2, title: 'Add Two Numbers', difficulty: 'medium' },
      { id: 'sum-two', number: 371, title: 'Sum of Two Integers', difficulty: 'medium' },
    ];
    expect(filterLibrary(rows, '  Two SUM ').map((row) => row.id)).toEqual(['two-sum', 'sum-two']);
  });

  it('Given an existing membership, When the same problem is added again, Then membership is unique', () => {
    const list: SavedProblemList = { id: 'favorite', name: 'Favorite', description: '', private: false, problemIds: ['two-sum'] };
    expect(putInList(list, 'two-sum')).toBe(list);
    expect(putInList(list, 'two-sum-bsts').problemIds).toEqual(['two-sum', 'two-sum-bsts']);
  });
});
