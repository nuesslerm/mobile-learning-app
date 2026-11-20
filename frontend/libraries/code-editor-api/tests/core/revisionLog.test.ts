import { describe, expect, it } from 'vitest';

import { createRevisionLog } from '../../src';

describe('the revision log', () => {
  it('given pushed change lists, when positions and ranges map from an older revision, then they follow every list since', () => {
    // Given a log with two edits: insert 2 at 0, then delete [5, 7)
    const log = createRevisionLog(10, 20, 8);
    log.push([{ from: 0, to: 0, insert: 'ab' }]);
    log.push([{ from: 5, to: 7, insert: '' }]);

    // Then mapping from the start follows both and from the middle follows the second
    expect(log.head()).toBe(12);
    expect(log.mapPos(5, 10, 1)).toBe(7 - 2);
    expect(log.mapRange({ from: 3, to: 9 }, 10)).toEqual({ from: 5, to: 9 });
    expect(log.mapPos(9, 11, 1)).toBe(7);
    expect(log.lengthAt(10)).toBe(20);
    expect(log.lengthAt(11)).toBe(22);
    expect(log.lengthAt(12)).toBe(20);
  });

  it('given more pushes than the depth, when the floor is read, then old revisions are gone and lengths stay right', () => {
    const log = createRevisionLog(0, 0, 3);
    for (let i = 0; i < 6; i += 1) log.push([{ from: 0, to: 0, insert: 'x' }]);
    expect(log.floor()).toBe(3);
    expect(log.lengthAt(2)).toBeNull();
    expect(log.lengthAt(3)).toBe(3);
    expect(log.lengthAt(6)).toBe(6);
  });

  it('given a reset, when read, then the log restarts at the new revision and length', () => {
    const log = createRevisionLog(0, 5, 4);
    log.push([{ from: 0, to: 1, insert: '' }]);
    log.reset(9, 12);
    expect([log.head(), log.floor(), log.lengthAt(9), log.lengthAt(8)]).toEqual([9, 9, 12, null]);
  });
});
