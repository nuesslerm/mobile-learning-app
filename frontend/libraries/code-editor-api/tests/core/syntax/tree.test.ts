import { describe, expect, it } from 'vitest';

import { errorRanges, isDamaged, isUnread, nodeAt, nodesInRange, pathAt } from '../../../src';
import { error, node, reply } from './helpers';

// Script 0-12 [ A 0-5, B 5-12 [ C 5-8, D 8-12 ] ]
const root = node('Script', 0, 12, [node('A', 0, 5), node('B', 5, 12, [node('C', 5, 8), node('D', 8, 12)])]);

describe('pathAt and nodeAt', () => {
  it('given two adjacent nodes, when the position is on their boundary, then the side names which one wins', () => {
    expect(nodeAt(root, 5, 1)?.type).toBe('C');
    expect(nodeAt(root, 5, -1)?.type).toBe('A');
  });

  it('given a position inside a leaf, then the path runs from the root to the leaf', () => {
    expect(pathAt(root, 9).map((n) => n.type)).toEqual(['Script', 'B', 'D']);
  });

  it('given a position at the end of the document, then the root still holds it', () => {
    expect(nodeAt(root, 12, 1)?.type).toBe('Script');
    expect(nodeAt(root, 12, -1)?.type).toBe('D');
  });

  it('given a position outside the root or no root, then there is no path', () => {
    expect(pathAt(root, 13)).toEqual([]);
    expect(nodeAt(null, 0)).toBeNull();
  });
});

describe('nodesInRange', () => {
  it('given a point on the boundary of two statements, then both come back with their parents, in document order', () => {
    expect(nodesInRange(root, { from: 5, to: 5 }).map((n) => n.type)).toEqual(['Script', 'A', 'B', 'C']);
  });

  it('given a range, then the nodes that overlap it come back and a node that only ends at its start does not', () => {
    expect(nodesInRange(root, { from: 5, to: 9 }).map((n) => n.type)).toEqual(['Script', 'B', 'C', 'D']);
  });

  it('given within, then only the nodes inside the range come back', () => {
    expect(nodesInRange(root, { from: 5, to: 12 }, { within: true }).map((n) => n.type)).toEqual(['B', 'C', 'D']);
  });
});

describe('errors and unread text', () => {
  const broken = node('Script', 0, 20, [node('For', 0, 8, [error(8)]), node('Pass', 9, 13), node('Bad', 14, 20, [node('Inner', 14, 16, [error(16)])])]);

  it('given error nodes, when their ranges are asked, then the outermost ones come back in order and a range filters them', () => {
    expect(errorRanges(broken)).toEqual([{ from: 8, to: 8 }, { from: 16, to: 16 }]);
    expect(errorRanges(broken, { from: 9, to: 20 })).toEqual([{ from: 16, to: 16 }]);
  });

  it('given a node with an error child, then it is damaged and its parent is not', () => {
    expect(isDamaged(broken.children?.[0] as never)).toBe(true);
    expect(isDamaged(broken)).toBe(false);
  });

  it('given a tree parsed to 3001 of 6000, when a position past 3001 is asked, then it is unread', () => {
    const value = reply('python', node('Script', 0, 3001), 6000, 3001);

    expect(isUnread(value, 3001)).toBe(false);
    expect(isUnread(value, 3002)).toBe(true);
  });
});
