import { describe, expect, it } from 'vitest';

import {
  INDENT_UNIT,
  dedent,
  indent,
  moveLeft,
  moveRight,
  smartNewline,
  toggleComment,
} from '../editing';
import { profileOf } from '../languageProfile';

const caret = (at: number) => ({ start: at, end: at });

describe('indent', () => {
  it('given a caret, when indent runs, then one level goes in at the caret and the caret ends after it', () => {
    expect(indent('a\nb', caret(1))).toEqual({ text: `a${INDENT_UNIT}\nb`, selection: caret(1 + INDENT_UNIT.length) });
  });

  it('given a selection over two lines, when indent runs, then both lines gain a level and the block stays selected', () => {
    const result = indent('a\nb', { start: 0, end: 3 });
    expect(result).toEqual({ text: '    a\n    b', selection: { start: 0, end: 11 } });
  });

  it('given a selection that starts mid-line, when indent runs, then the whole first line is indented', () => {
    expect(indent('abc\ndef', { start: 2, end: 5 }).text).toBe('    abc\n    def');
  });
});

describe('dedent', () => {
  it('given a line with one full level, when dedent runs, then the level goes and the caret keeps its place in the text', () => {
    expect(dedent('    x', caret(5))).toEqual({ text: 'x', selection: caret(1) });
  });

  it('given a line with two spaces and one with six, when dedent runs, then at most one level goes from each', () => {
    expect(dedent('  a\n      b', { start: 0, end: 10 }).text).toBe('a\n  b');
  });

  it('given a tab indent, when dedent runs, then it stays: only spaces count as a level', () => {
    expect(dedent('\tx', caret(2))).toEqual({ text: '\tx', selection: caret(2) });
  });

  it('given a caret inside the indent that goes, when dedent runs, then the caret lands at the line start', () => {
    expect(dedent('        x', caret(2)).selection).toEqual(caret(0));
  });

  it('given a selection, when dedent runs, then the whole transformed block stays selected', () => {
    expect(dedent('    a\n    b', { start: 0, end: 9 })).toEqual({ text: 'a\nb', selection: { start: 0, end: 3 } });
  });
});

describe('toggleComment', () => {
  it('given plain lines, when it runs, then each non-blank line gets "# " and blank lines stay blank', () => {
    expect(toggleComment('a\n\nb', { start: 0, end: 4 }).text).toBe('# a\n\n# b');
  });

  it('given every line commented, when it runs, then the markers go, with or without the space', () => {
    expect(toggleComment('# a\n#b', { start: 0, end: 6 }).text).toBe('a\nb');
  });

  it('given a mix of commented and plain lines, when it runs, then every line is commented, the commented one twice', () => {
    expect(toggleComment('a\n# b', { start: 0, end: 5 }).text).toBe('# a\n# # b');
  });

  it('given an indented line, when it runs, then the marker goes after the indent', () => {
    expect(toggleComment('    x', caret(5)).text).toBe('    # x');
  });

  it('given a caret after the text of a line, when it comments and uncomments, then the caret follows the text it was after', () => {
    const commented = toggleComment('x', caret(1));
    expect(commented).toEqual({ text: '# x', selection: caret(3) });
    expect(toggleComment(commented.text, commented.selection)).toEqual({ text: 'x', selection: caret(1) });
  });

  // pinned, known quirk: the caret moves by the length of the marker even when it sat before the marker's place
  it('given a caret at the start of an indented line, when it runs, then the caret moves into the indent (pinned, known quirk)', () => {
    expect(toggleComment('    x', caret(0))).toEqual({ text: '    # x', selection: caret(2) });
  });
});

describe('smartNewline', () => {
  it('given a caret at the end of an indented line, when it runs, then the next line keeps the indent', () => {
    expect(smartNewline('    y = 1', caret(9))).toEqual({ text: '    y = 1\n    ', selection: caret(14) });
  });

  it('given a line that ends in a colon, when it runs, then the next line goes one level deeper', () => {
    expect(smartNewline('if x:', caret(5))).toEqual({ text: 'if x:\n    ', selection: caret(10) });
  });

  it('given a colon followed by a trailing space, when it runs, then it still opens a block', () => {
    expect(smartNewline('if x: ', caret(6)).text).toBe('if x: \n    ');
  });

  it('given a selection, when it runs, then the selection is replaced by the line break', () => {
    expect(smartNewline('abXYcd', { start: 2, end: 4 })).toEqual({ text: 'ab\ncd', selection: caret(3) });
  });

  // pinned, known quirk: the colon test looks at the whole line, so a break in the middle of a block opener indents the rest
  it('given a caret in the middle of a line that ends in a colon, when it runs, then the rest goes one level deeper (pinned, known quirk)', () => {
    expect(smartNewline('if x:', caret(2)).text).toBe('if\n     x:');
  });
});

describe('moveLeft and moveRight', () => {
  it('given a caret, when it moves, then it moves one character and stops at the ends', () => {
    expect(moveLeft('abc', caret(3))).toEqual(caret(2));
    expect(moveLeft('abc', caret(0))).toEqual(caret(0));
    expect(moveRight('abc', caret(0))).toEqual(caret(1));
    expect(moveRight('abc', caret(3))).toEqual(caret(3));
  });

  it('given a selection of either direction, when it moves, then it collapses to the edge on that side', () => {
    expect(moveLeft('abcdef', { start: 2, end: 4 })).toEqual(caret(2));
    expect(moveLeft('abcdef', { start: 4, end: 2 })).toEqual(caret(2));
    expect(moveRight('abcdef', { start: 2, end: 4 })).toEqual(caret(4));
    expect(moveRight('abcdef', { start: 4, end: 2 })).toEqual(caret(4));
  });
});

describe('with another indent unit', () => {
  const two = '  ';
  const eight = '        ';

  it('given a unit of two, when indent runs at a caret and over lines, then two spaces go in', () => {
    expect(indent('x', caret(0), two)).toEqual({ text: '  x', selection: caret(2) });
    expect(indent('a\nb', { start: 0, end: 3 }, two)).toEqual({ text: '  a\n  b', selection: { start: 0, end: 7 } });
  });

  it('given a unit of eight, when indent runs, then eight spaces go in', () => {
    expect(indent('x', caret(0), eight)).toEqual({ text: `${eight}x`, selection: caret(8) });
  });

  it('given a unit of two, when dedent runs on lines of four and one space, then at most two spaces go from each', () => {
    expect(dedent('    a\n b', { start: 0, end: 7 }, two).text).toBe('  a\nb');
  });

  it('given a unit of eight, when dedent runs on a line of four and one of eight spaces, then up to eight go from each', () => {
    expect(dedent('    a\n        b', { start: 0, end: 15 }, eight).text).toBe('a\nb');
  });

  it('given a unit of two, when newline runs after a colon, then the next line is two spaces deeper', () => {
    expect(smartNewline('if x:', caret(5), undefined, two)).toEqual({ text: 'if x:\n  ', selection: caret(8) });
  });

  it('given a unit of eight and an indented line that opens a block, when newline runs, then the line goes eight deeper than its own indent', () => {
    expect(smartNewline('  if x:', caret(7), undefined, eight).text).toBe(`  if x:\n  ${eight}`);
  });

  it('given a unit of two and the caret between a bracket pair in JavaScript, when newline runs, then the closer drops to its own line', () => {
    expect(smartNewline('f()', caret(2), profileOf('javascript'), two)).toEqual({ text: 'f(\n  \n)', selection: caret(5) });
  });

  it('given no unit, when indent, dedent and newline run, then they use four spaces as before', () => {
    expect(indent('x', caret(0)).text).toBe('    x');
    expect(dedent('    x', caret(5)).text).toBe('x');
    expect(smartNewline('if x:', caret(5)).text).toBe('if x:\n    ');
  });
});
