import { describe, expect, it } from 'vitest';

import {
  DEFAULT_SYMBOL_GROUPS,
  SYMBOL_GROUP_IDS,
  groupCells,
  groupLabel,
  isDefaultGroups,
  moveGroup,
  normalizeGroup,
  resetGroups,
  setGroupChars,
  symbolGroupsSchema,
  visibleGroups,
} from '../symbolGroups';

const chars = (groups: readonly { chars: string }[]) => groups.map((group) => group.chars);

// the characters the classic rail's Paren, Bracket, Brace, Quote, Apostrophe, Colon, Equals, Underscore and Dot keys type
const CLASSIC_CHARACTER_KEYS = ['(', '[', '{', '"', "'", ':', '=', '_', '.'];

describe('DEFAULT_SYMBOL_GROUPS', () => {
  it('Given the defaults, When read in order, Then the eight strings are the recording\'s and the ids are g1 to g8', () => {
    expect(chars(DEFAULT_SYMBOL_GROUPS)).toEqual(['#$^%', '&|?!', '\\@~_', '+-*/', '\'"`=', '{}[]', '()<>', '.:,;']);
    expect(DEFAULT_SYMBOL_GROUPS.map((group) => group.id)).toEqual([...SYMBOL_GROUP_IDS]);
  });

  it('Given every character of the classic rail\'s character keys, When looked up in the defaults, Then each one is there', () => {
    const all = DEFAULT_SYMBOL_GROUPS.map((group) => group.chars).join('');

    for (const key of CLASSIC_CHARACTER_KEYS) expect(all).toContain(key);
  });

  it('Given the defaults, When each group is counted, Then none is over four characters and none is empty', () => {
    for (const group of DEFAULT_SYMBOL_GROUPS) {
      expect(Array.from(group.chars).length).toBeGreaterThan(0);
      expect(Array.from(group.chars).length).toBeLessThanOrEqual(4);
    }
  });
});

describe('normalizeGroup', () => {
  it('Given more than four characters, When normalised, Then the first four stay', () => {
    expect(normalizeGroup('abcdef')).toBe('abcd');
  });

  it('Given emoji, When normalised, Then each counts as one character and the cap is four of them', () => {
    expect('\u{1F600}'.length).toBe(2);
    expect(normalizeGroup('\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}')).toBe('\u{1F600}'.repeat(4));
    expect(normalizeGroup('a\u{1F600}')).toBe('a\u{1F600}');
  });

  it('Given spaces, tabs, new lines, control and invisible characters, When normalised, Then they are dropped before the cap is counted', () => {
    expect(normalizeGroup(' a\tb\nc\u0000d e​f ')).toBe('abcd');
  });

  it('Given an empty string, When normalised, Then it stays empty, which is how a group is hidden', () => {
    expect(normalizeGroup('')).toBe('');
    expect(normalizeGroup('   ')).toBe('');
  });

  it('Given the same character twice, When normalised, Then both stay', () => {
    expect(normalizeGroup('((')).toBe('((');
  });
});

describe('visibleGroups', () => {
  it('Given a group emptied, When the visible groups are read, Then that group is left out and the others keep their order', () => {
    const groups = setGroupChars(DEFAULT_SYMBOL_GROUPS, 'g3', '');

    expect(visibleGroups(groups).map((group) => group.id)).toEqual(['g1', 'g2', 'g4', 'g5', 'g6', 'g7', 'g8']);
  });
});

describe('groupCells', () => {
  it('Given four characters, When laid out, Then two rows of two', () => {
    expect(groupCells('#$^%')).toEqual([
      ['#', '$'],
      ['^', '%'],
    ]);
  });

  it('Given one to three characters or none, When laid out, Then the rows are as full as they can be', () => {
    expect(groupCells('{')).toEqual([['{']]);
    expect(groupCells('{}')).toEqual([['{', '}']]);
    expect(groupCells('{}[')).toEqual([['{', '}'], ['[']]);
    expect(groupCells('')).toEqual([]);
  });

  it('Given an emoji, When laid out, Then it is one cell', () => {
    expect(groupCells('a\u{1F600}b')).toEqual([['a', '\u{1F600}'], ['b']]);
  });
});

describe('moveGroup and groupLabel', () => {
  it('Given group 2 moved below group 3, When read, Then the order is the recording\'s and the labels follow the places', () => {
    const moved = moveGroup(DEFAULT_SYMBOL_GROUPS, 1, 2);

    expect(chars(moved).slice(0, 3)).toEqual(['#$^%', '\\@~_', '&|?!']);
    expect(moved.map((group) => group.id).slice(0, 3)).toEqual(['g1', 'g3', 'g2']);
    expect(moved.map((_group, index) => groupLabel(index))).toEqual(['Group 1', 'Group 2', 'Group 3', 'Group 4', 'Group 5', 'Group 6', 'Group 7', 'Group 8']);
  });

  it('Given a move to the same place or out of range, When applied, Then the list itself comes back', () => {
    expect(moveGroup(DEFAULT_SYMBOL_GROUPS, 2, 2)).toBe(DEFAULT_SYMBOL_GROUPS);
    expect(moveGroup(DEFAULT_SYMBOL_GROUPS, -1, 2)).toBe(DEFAULT_SYMBOL_GROUPS);
    expect(moveGroup(DEFAULT_SYMBOL_GROUPS, 0, 8)).toBe(DEFAULT_SYMBOL_GROUPS);
  });

  it('Given a move, When applied, Then the original list is not changed', () => {
    const before = chars(DEFAULT_SYMBOL_GROUPS);

    moveGroup(DEFAULT_SYMBOL_GROUPS, 0, 7);

    expect(chars(DEFAULT_SYMBOL_GROUPS)).toEqual(before);
  });
});

describe('isDefaultGroups and resetGroups', () => {
  it('Given the defaults, When asked, Then they are the defaults', () => {
    expect(isDefaultGroups(DEFAULT_SYMBOL_GROUPS)).toBe(true);
  });

  it('Given a move, When asked, Then it is no longer the defaults, and after resetGroups it is again', () => {
    const moved = moveGroup(DEFAULT_SYMBOL_GROUPS, 1, 2);

    expect(isDefaultGroups(moved)).toBe(false);
    expect(isDefaultGroups(resetGroups())).toBe(true);
  });

  it('Given a group moved and moved back, When asked, Then it is the defaults again', () => {
    expect(isDefaultGroups(moveGroup(moveGroup(DEFAULT_SYMBOL_GROUPS, 1, 2), 2, 1))).toBe(true);
  });

  it('Given a group edited, When asked, Then it is not the defaults', () => {
    expect(isDefaultGroups(setGroupChars(DEFAULT_SYMBOL_GROUPS, 'g1', '#'))).toBe(false);
  });

  it('Given resetGroups called twice, When one result is edited, Then the other and the defaults are untouched', () => {
    const first = resetGroups();
    const second = resetGroups();

    expect(first).toEqual(second);
    expect(first).not.toBe(DEFAULT_SYMBOL_GROUPS);
    expect(first[0]).not.toBe(DEFAULT_SYMBOL_GROUPS[0]);
  });
});

describe('setGroupChars', () => {
  it('Given text longer than four characters for a group, When set, Then it is normalised and the other groups stay', () => {
    const groups = setGroupChars(DEFAULT_SYMBOL_GROUPS, 'g2', 'abcdef');

    expect(chars(groups)).toEqual(['#$^%', 'abcd', '\\@~_', '+-*/', '\'"`=', '{}[]', '()<>', '.:,;']);
  });

  it('Given the same character in two groups, When set, Then both keep it', () => {
    const groups = setGroupChars(DEFAULT_SYMBOL_GROUPS, 'g1', '()');

    expect(chars(groups)[0]).toBe('()');
    expect(chars(groups)[6]).toBe('()<>');
  });
});

describe('symbolGroupsSchema', () => {
  it('Given the defaults and a reordered copy, When parsed, Then both are accepted', () => {
    expect(symbolGroupsSchema.safeParse(DEFAULT_SYMBOL_GROUPS).success).toBe(true);
    expect(symbolGroupsSchema.safeParse(moveGroup(DEFAULT_SYMBOL_GROUPS, 0, 7)).success).toBe(true);
  });

  it('Given seven or nine entries, When parsed, Then it is refused', () => {
    expect(symbolGroupsSchema.safeParse(DEFAULT_SYMBOL_GROUPS.slice(0, 7)).success).toBe(false);
    expect(symbolGroupsSchema.safeParse([...DEFAULT_SYMBOL_GROUPS, { id: 'g1', chars: 'a' }]).success).toBe(false);
  });

  it('Given two entries with one id, When parsed, Then it is refused', () => {
    const duplicated = DEFAULT_SYMBOL_GROUPS.map((group, index) => (index === 7 ? { ...group, id: 'g1' } : group));

    expect(symbolGroupsSchema.safeParse(duplicated).success).toBe(false);
  });

  it('Given an id that is not g1 to g8, When parsed, Then it is refused', () => {
    const renamed = DEFAULT_SYMBOL_GROUPS.map((group, index) => (index === 0 ? { ...group, id: 'g9' } : group));

    expect(symbolGroupsSchema.safeParse(renamed).success).toBe(false);
  });

  it('Given characters over the cap or with whitespace, When parsed, Then it is refused', () => {
    const tooLong = DEFAULT_SYMBOL_GROUPS.map((group, index) => (index === 0 ? { ...group, chars: 'abcde' } : group));
    const spaced = DEFAULT_SYMBOL_GROUPS.map((group, index) => (index === 0 ? { ...group, chars: 'a b' } : group));

    expect(symbolGroupsSchema.safeParse(tooLong).success).toBe(false);
    expect(symbolGroupsSchema.safeParse(spaced).success).toBe(false);
  });

  it('Given an empty group, When parsed, Then it is accepted, since empty hides the group', () => {
    expect(symbolGroupsSchema.safeParse(setGroupChars(DEFAULT_SYMBOL_GROUPS, 'g4', '')).success).toBe(true);
  });

  it('Given something that is not a list, When parsed, Then it is refused', () => {
    expect(symbolGroupsSchema.safeParse('groups').success).toBe(false);
    expect(symbolGroupsSchema.safeParse(null).success).toBe(false);
  });
});
