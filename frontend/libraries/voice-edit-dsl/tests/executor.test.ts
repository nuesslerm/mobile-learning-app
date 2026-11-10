import { describe, it, expect } from 'vitest';
import { applyCommands } from '../src/use-cases/executor';
import type { EditCommand } from '../src/contracts/schema';

const sample = 'function sum(arr) {\n  let total = 0;\n  return total;\n}';

describe('applyCommands', () => {
  it('replaces a range', () => {
    const commands: EditCommand[] = [{ type: 'replaceRange', start: 0, end: 8, newText: 'const fn' }];
    const result = applyCommands(sample, commands);
    expect(result.text.startsWith('const fn')).toBe(true);
  });

  it('inserts text at a position', () => {
    const commands: EditCommand[] = [{ type: 'insertAt', position: 0, text: '// comment\n' }];
    const result = applyCommands(sample, commands);
    expect(result.text.startsWith('// comment\n')).toBe(true);
  });

  it('deletes a range', () => {
    const commands: EditCommand[] = [{ type: 'deleteRange', start: 0, end: 9 }];
    const result = applyCommands(sample, commands);
    expect(result.text).not.toContain('function ');
  });

  it('renames a symbol', () => {
    const commands: EditCommand[] = [{ type: 'renameSymbol', position: sample.indexOf('total'), newName: 'sum' }];
    const result = applyCommands(sample, commands);
    expect(result.text).toContain('let sum = 0');
    expect(result.text).toContain('return sum');
  });

  it('moves a range', () => {
    const text = 'abcdef';
    const commands: EditCommand[] = [{ type: 'moveRange', start: 1, end: 3, insertAt: 3 }];
    const result = applyCommands(text, commands);
    expect(result.text).toBe('adebcf');
  });

  it('wraps a range', () => {
    const text = 'hello';
    const commands: EditCommand[] = [{ type: 'wrapWith', start: 0, end: 5, before: '[', after: ']' }];
    const result = applyCommands(text, commands);
    expect(result.text).toBe('[hello]');
  });

  it('extracts a block', () => {
    const text = '  console.log(1);';
    const commands: EditCommand[] = [{ type: 'extractBlock', start: 2, end: 17, newName: 'logOne' }];
    const result = applyCommands(text, commands);
    expect(result.text).toContain('logOne();');
    expect(result.text).toContain('function logOne()');
  });

  it('applies a multi-step pipeline', () => {
    const commands: EditCommand[] = [
      { type: 'renameSymbol', position: sample.indexOf('total'), newName: 'sum' },
      { type: 'replaceRange', start: 0, end: 8, newText: 'const fn' },
    ];
    const result = applyCommands(sample, commands);
    expect(result.text).toContain('let sum = 0');
    expect(result.text.startsWith('const fn')).toBe(true);
  });
});

describe('applyCommands with multiple commands', () => {
  // Every command's offsets refer to the code the planner was given, so a
  // second command must not be applied at face value against text the first
  // one already resized. This is the exact shape that corrupted a real
  // re-indent live: two whitespace fixes, the later one landing five
  // characters off and eating `l, r `.
  const code = 'class S:\n     def f(self):\n           """Doc."""\n     x = 1\n';

  it('applies a later command at offsets rebased past an earlier one', () => {
    // Given two fixes whose offsets were both measured on `code`
    const docStart = code.indexOf('"""') - 11;
    const xStart = code.indexOf('x = 1') - 5;
    const commands = [
      { type: 'replaceRange' as const, start: docStart, end: docStart + 11, newText: '        ' },
      { type: 'replaceRange' as const, start: xStart, end: xStart + 5, newText: '        ' },
    ];

    // When they are applied together
    const { text } = applyCommands(code, commands);

    // Then each landed on its own whitespace, leaving the code intact
    expect(text).toBe('class S:\n     def f(self):\n        """Doc."""\n        x = 1\n');
  });

  it('rejects two commands that cover the same span', () => {
    // Given two commands whose original-text ranges overlap
    const commands = [
      { type: 'replaceRange' as const, start: 0, end: 8, newText: 'class T:' },
      { type: 'replaceRange' as const, start: 6, end: 8, newText: 'X:' },
    ];

    // When they are applied together
    // Then the conflict is reported rather than silently mangled
    expect(() => applyCommands(code, commands)).toThrow(/overlaps an earlier one/);
  });
});

describe('offset snapping', () => {
  const code = 'class S:\n    def f(self):\n        while left < right:\n            pass\n';

  it('snaps a block insert that would split a word', () => {
    // Given a planner offset three characters early -- inside `right:` --
    // which is exactly what split `while left < righ` / `t:` live
    const bad = code.indexOf('while left < right:') + 'while left < righ'.length;
    const commands = [
      { type: 'insertAt' as const, position: bad, text: '        # note\n' },
    ];

    // When the edit is applied
    const { text } = applyCommands(code, commands);

    // Then it lands on a line boundary instead of inside the token
    expect(text).toContain('while left < right:');
    expect(text).toContain('# note');
  });

  it('leaves a single-line edit exactly where it was asked for', () => {
    // Given a mid-line replacement, where the middle of a line is the point
    const start = code.indexOf('left');
    const commands = [
      { type: 'replaceRange' as const, start, end: start + 4, newText: 'lo' },
    ];

    // When it is applied
    const { text } = applyCommands(code, commands);

    // Then no snapping happened
    expect(text).toContain('while lo < right:');
  });
});

describe('block inserts land on a line boundary', () => {
  it('snaps an insert that lands between an operator and its operand', () => {
    // Given the offset that produced `right -=` / `return True1` live -- a
    // space sits before it, so a word-char test finds nothing wrong
    const code = 'def f():\n    right -= 1\n';
    const bad = code.indexOf('right -= 1') + 'right -= '.length;
    const commands = [
      { type: 'insertAt' as const, position: bad, text: '    return True\n' },
    ];

    // When the block insert is applied
    const { text } = applyCommands(code, commands);

    // Then the statement it landed in survives intact
    expect(text).toContain('right -= 1');
    expect(text).not.toContain('True1');
  });
});

describe('a block insert aimed at the first token of a line goes above that line', () => {
  it('puts "before the while loop" above the loop header, not inside it', () => {
    // Given the offset the planner produced live for "before the while loop,
    // replace s ..." -- the first non-blank column of the `while` line
    const code = 'def f(s):\n    right = 1\n    while left < right:\n        pass\n';
    const position = code.indexOf('while');
    const commands = [
      { type: 'insertAt' as const, position, text: "    s = s.lower()\n" },
    ];

    // When the block insert is applied
    const { text } = applyCommands(code, commands);

    // Then the new line sits between the assignment and the loop, and the
    // loop header is untouched
    expect(text).toBe('def f(s):\n    right = 1\n    s = s.lower()\n    while left < right:\n        pass\n');
  });

  it('still brings its own newline when the block has none', () => {
    // Given a block without a trailing newline aimed at a line's first token
    const code = 'def f():\n    while x:\n        pass\n';
    const commands = [
      { type: 'insertAt' as const, position: code.indexOf('while'), text: '    y = 1\n    z = 2' },
    ];

    // When it is applied
    const { text } = applyCommands(code, commands);

    // Then the loop header starts on its own line
    expect(text).toContain('    z = 2\n    while x:');
  });
});

describe('a block insert into an indented stub line fills that line', () => {
  it('keeps the stub indentation for a bare first line', () => {
    // Given the starter's empty, indented method body and a planner insert one
    // column into it whose first line carries no indentation of its own --
    // exactly the shape that produced `left = 0` in column 0 live
    const code = 'class Solution:\n    def f(self, s: str) -> bool:\n        ';
    const commands = [
      { type: 'insertAt' as const, position: code.length - 7, text: 'left = 0\n        right = 1\n' },
    ];

    // When the block insert is applied
    const { text } = applyCommands(code, commands);

    // Then both lines sit in the method body
    expect(text).toBe('class Solution:\n    def f(self, s: str) -> bool:\n        left = 0\n        right = 1\n');
  });

  it('does not double the indentation of a first line that brings its own', () => {
    // Given the starter's empty, indented method body and the deployed planner's
    // command for "declare left at zero, right at the last index, and best at zero"
    const code = 'class Solution:\n    def maxArea(self, height: List[int]) -> int:\n        ';
    const commands = [
      { type: 'insertAfterLine' as const, line: 3, text: '        left = 0\n        right = len(height) - 1\n        best = 0' },
    ];

    // When the block insert is applied
    const { text } = applyCommands(code, commands);

    // Then every line sits one level into the method body
    expect(text).toBe(
      'class Solution:\n    def maxArea(self, height: List[int]) -> int:\n        left = 0\n        right = len(height) - 1\n        best = 0'
    );
  });
});

describe('a block replace over an indented stub keeps the indentation', () => {
  it('re-indents a bare first line when the snap swallowed the indentation', () => {
    // Given the starter's indented stub body and the planner's live command
    // for "declare a left pointer ...": a replace from one column into the
    // indentation to past the end, with a bare first line
    const code = 'class Solution:\n    def f(self, s: str) -> bool:\n        ';
    const commands = [
      { type: 'replaceRange' as const, start: 61, end: 69, newText: 'left = 0\n        right = 1\n' },
    ];

    // When the block replace is applied
    const { text } = applyCommands(code, commands);

    // Then the first line is indented like the rest of the body
    expect(text).toBe('class Solution:\n    def f(self, s: str) -> bool:\n        left = 0\n        right = 1\n');
  });
});

describe('a block insert at the start of a line stays on its own lines', () => {
  it('does not glue the following line onto a block that lacks a trailing newline', () => {
    // Given the planner's live command for "add return true after the while
    // loop": a block starting with a newline, ending without one, aimed at
    // the start of an existing line
    const code = 'def f():\n    a = 1\n    b = 2';
    const commands = [
      { type: 'insertAt' as const, position: code.indexOf('    b = 2'), text: '\n    return True' },
    ];

    // When it is applied
    const { text } = applyCommands(code, commands);

    // Then the block sits on its own line and `b = 2` keeps its line
    expect(text).toBe('def f():\n    a = 1\n    return True\n    b = 2');
  });
});

describe('a bare first line above indented lines takes the landing line\'s indentation', () => {
  it('re-indents a replace that started at the pass line\'s first column', () => {
    // Given the planner's live command: replace the first four characters of
    // the `pass` line (it meant `pass`) with an if-block whose first line is bare
    const code = 'def f():\n    while a:\n        pass\n';
    const start = code.indexOf('        pass');
    const commands = [
      { type: 'replaceRange' as const, start, end: start + 4, newText: 'if a == b:\n            return False\n        a += 1' },
    ];

    // When it is applied
    const { text } = applyCommands(code, commands);

    // Then the if sits in the loop body like the lines under it
    expect(text).toBe('def f():\n    while a:\n        if a == b:\n            return False\n        a += 1\n');
  });

  it('re-indents a bare first line inserted at the start of an indented line', () => {
    // Given a bare `return True` block with indented follow-up, aimed at a line start
    const code = 'def f():\n    a = 1\n    b = 2\n';
    const commands = [
      { type: 'insertAt' as const, position: code.indexOf('    b = 2'), text: 'if a:\n        pass\n' },
    ];

    // When it is applied
    const { text } = applyCommands(code, commands);

    // Then the block is indented like the line it pushed down
    expect(text).toBe('def f():\n    a = 1\n    if a:\n        pass\n    b = 2\n');
  });
});

describe('a block insert at the very end of an indented stub fills that line', () => {
  it('keeps the stub indentation when the planner points at end of file', () => {
    // Given the starter whose last line is the indented empty body and the
    // planner's live command: insert at code.length with a bare first line
    const code = 'class Solution:\n    def f(self, s: str) -> bool:\n        ';
    const commands = [
      { type: 'insertAt' as const, position: code.length, text: 'left = 0\n        right = len(s) - 1' },
    ];

    // When it is applied
    const { text } = applyCommands(code, commands);

    // Then the first line sits in the body, not on a blank line in column 0
    expect(text).toBe('class Solution:\n    def f(self, s: str) -> bool:\n        left = 0\n        right = len(s) - 1');
  });
});

describe('renameSymbol lands on an identifier', () => {
  const code = 'class Solution:\n    def f(self):\n        low, right = 0, 1\n        while low < right:\n            pass\n';

  it('snaps a position that fell on whitespace onto the nearby identifier', () => {
    // Given the offset that wrecked a real buffer: one space to the left of
    // `right`, which made wordRangeAt return an empty range
    const onSpace = code.indexOf('low, right') + 'low,'.length;
    const commands = [
      { type: 'renameSymbol' as const, position: onSpace, newName: 'high' },
    ];

    // When the rename is applied
    const { text } = applyCommands(code, commands);

    // Then it renamed the identifier, everywhere, and nothing else
    expect(text).toContain('low, high = 0, 1');
    expect(text).toContain('while low < high:');
    expect(text).toContain('class Solution:');
    expect(text).not.toContain('highclass');
  });

  it('refuses a rename with no identifier anywhere on the line', () => {
    // Given a position on a blank line, where snapping has nothing to find
    const blank = 'a = 1\n\nb = 2\n';
    const commands = [
      { type: 'renameSymbol' as const, position: 6, newName: 'x' },
    ];

    // When it is applied
    // Then it refuses rather than rewriting every word boundary in the file
    expect(() => applyCommands(blank, commands)).toThrow(/not on an identifier/);
  });
});

describe('block replace snaps outward to whole lines', () => {
  it('replaces the pass line instead of appending after the word', () => {
    // Given the planner's start on the `p` of `pass` -- closer to the line's
    // end than its start, which is what glued `passif ...` together live
    const code = 'def f():\n    while a < b:\n        pass\n';
    const start = code.indexOf('pass');
    const commands = [
      {
        type: 'replaceRange' as const,
        start,
        end: start + 'pass'.length,
        newText: '        if a == b:\n            return False\n        a += 1',
      },
    ];

    // When it is applied
    const { text } = applyCommands(code, commands);

    // Then `pass` is gone and the block stands in its place
    expect(text).not.toContain('pass');
    expect(text).toContain('    while a < b:\n        if a == b:\n            return False\n        a += 1');
  });

  it('inserts a mid-line block after the statement it landed in', () => {
    // Given a block insert whose position fell inside `right -= 1`
    const code = 'def f():\n    right -= 1\n';
    const position = code.indexOf('-= 1') + 1;
    const commands = [{ type: 'insertAt' as const, position, text: '\n    return True' }];

    // When it is applied
    const { text } = applyCommands(code, commands);

    // Then the statement is intact and the block follows it
    expect(text).toBe('def f():\n    right -= 1\n    return True\n');
  });
});

describe('block inserts at a line end bring their own newline', () => {
  it('does not glue an inserted block onto the statement before it', () => {
    // Given an insert positioned right after `pass` at the end of the buffer,
    // with text that does not start with a newline -- the shape behind
    // `passif s[left] != s[right]:` live
    const code = 'def f():\n    while a < b:\n        pass';
    const commands = [
      { type: 'insertAt' as const, position: code.length, text: '        if a == b:\n            return False' },
    ];

    // When it is applied
    const { text } = applyCommands(code, commands);

    // Then the block starts on its own line
    expect(text).toBe('def f():\n    while a < b:\n        pass\n        if a == b:\n            return False');
  });
});
