import { describe, expect, it } from 'vitest';

import { applyCommands } from '../src/use-cases/executor';
import {
  numberLines,
  resolveLineCommand,
} from '../src/use-cases/lineCommands';
import { validateCommands } from '../src/use-cases/validator';

const code =
  'class Solution:\n    def f(self, s: str) -> bool:\n        left = 0\n        right = len(s) - 1\n        while left < right:\n            pass';

describe('line-anchored commands', () => {
  it('replaceLines swaps whole lines and keeps the surrounding ones byte-for-byte', () => {
    // Given the planner replacing the `pass` line (line 6) with a block
    const { text } = applyCommands(code, [
      {
        type: 'replaceLines',
        startLine: 6,
        endLine: 6,
        newText:
          '            if s[left] != s[right]:\n                return False',
      },
    ]);

    // Then only that line changed
    expect(text).toBe(
      code.replace(
        '            pass',
        '            if s[left] != s[right]:\n                return False'
      )
    );
  });

  it('insertAfterLine lands a block on its own lines after the named line, and before line 1 for 0', () => {
    expect(
      applyCommands(code, [
        { type: 'insertAfterLine', line: 4, text: '        s = s.lower()' },
      ]).text
    ).toBe(
      code.replace(
        '        right = len(s) - 1\n',
        '        right = len(s) - 1\n        s = s.lower()\n'
      )
    );
    expect(
      applyCommands(code, [
        { type: 'insertAfterLine', line: 0, text: '# header' },
      ]).text
    ).toBe(`# header\n${code}`);
    // After the unterminated last line: the block brings its own newline
    expect(
      applyCommands(code, [
        { type: 'insertAfterLine', line: 6, text: '        return True' },
      ]).text
    ).toBe(`${code}\n        return True`);
  });

  it('deleteLines removes the lines and their newline, including at end of file', () => {
    expect(
      applyCommands(code, [{ type: 'deleteLines', startLine: 3, endLine: 4 }])
        .text
    ).toBe(code.replace('        left = 0\n        right = len(s) - 1\n', ''));
    expect(
      applyCommands(code, [{ type: 'deleteLines', startLine: 6, endLine: 6 }])
        .text
    ).toBe(code.replace('\n            pass', ''));
  });

  it('clamps out-of-range lines instead of throwing', () => {
    expect(
      resolveLineCommand(code, {
        type: 'replaceLines',
        startLine: 9,
        endLine: 12,
        newText: 'x',
      })
    ).toEqual({
      type: 'replaceRange',
      start: code.lastIndexOf('\n') + 1,
      end: code.length,
      newText: 'x',
    });
  });

  it('validates line commands against the same size limits and overlap rules', () => {
    expect(
      validateCommands(code, [
        {
          type: 'replaceLines',
          startLine: 6,
          endLine: 6,
          newText: 'a\nb\nc\nd\ne\nf\ng',
        },
      ]).valid
    ).toBe(false);
    expect(
      validateCommands(code, [
        { type: 'replaceLines', startLine: 5, endLine: 6, newText: 'x' },
        { type: 'deleteLines', startLine: 6, endLine: 6 },
      ]).valid
    ).toBe(false);
    expect(
      validateCommands(code, [
        { type: 'insertAfterLine', line: 6, text: '        return True' },
      ]).valid
    ).toBe(true);
  });

  it('numbers lines the way the v2 prompt shows them', () => {
    expect(numberLines('a\nb')).toBe('1| a\n2| b');
  });
});

describe('normalizeVoiceEditResponse', () => {
  it('folds newText on insertAfterLine (and text on replaceLines) to the schema field', async () => {
    // Given a model reply that swapped the two block field names
    const { normalizeVoiceEditResponse } =
      await import('../src/use-cases/normalizeResponse');
    const raw = {
      transcript: 't',
      alternatives: [
        {
          id: 'a',
          commands: [
            { type: 'insertAfterLine', line: 3, newText: '        left = 0' },
            { type: 'replaceLines', startLine: 1, endLine: 1, text: 'x' },
          ],
        },
      ],
    };
    // When normalised
    const fixed = normalizeVoiceEditResponse(raw) as typeof raw;
    // Then both commands carry the schema's field names
    expect(fixed.alternatives[0]!.commands).toEqual([
      { type: 'insertAfterLine', line: 3, text: '        left = 0' },
      { type: 'replaceLines', startLine: 1, endLine: 1, newText: 'x' },
    ]);
  });
});
