import { describe, it, expect } from 'vitest';
import { DEFAULT_EDIT_SIZE_LIMITS, editSizeLimitsFor, validateCommands } from '../src/use-cases/validator';
import type { EditCommand } from '../src/contracts/schema';

const text = 'hello world';

describe('validateCommands', () => {
  it('passes for valid commands', () => {
    const commands: EditCommand[] = [{ type: 'replaceRange', start: 0, end: 5, newText: 'hi' }];
    expect(validateCommands(text, commands)).toEqual({ valid: true });
  });

  it('fails when start > end', () => {
    const commands: EditCommand[] = [{ type: 'replaceRange', start: 5, end: 0, newText: 'x' }];
    const result = validateCommands(text, commands);
    expect(result.valid).toBe(false);
    expect(result.reason).toContain('start');
  });

  it('accepts an end past the last character as "to the end"', () => {
    // Given a command whose end overshoots the buffer, as planners routinely
    // emit -- a live run produced end 69 against 68 characters in all three
    // alternatives at once, leaving nothing acceptable for an instruction the
    // model had understood correctly
    const commands: EditCommand[] = [{ type: 'deleteRange', start: 0, end: 100 }];

    // When it is validated
    const result = validateCommands(text, commands);

    // Then it is clamped rather than refused: an end past the last character
    // has only one possible meaning
    expect(result.valid).toBe(true);
  });

  it('fails when two commands cover the same characters', () => {
    // Given two commands whose ranges overlap in the original text
    const commands: EditCommand[] = [
      { type: 'deleteRange', start: 0, end: 11 },
      { type: 'replaceRange', start: 6, end: 11, newText: 'x' },
    ];

    // When they are validated
    const result = validateCommands(text, commands);

    // Then the second is reported: both offsets were written against the
    // untouched code, so they cannot both mean what the planner intended
    expect(result.valid).toBe(false);
    expect(result.reason).toContain('overlaps');
    expect(result.failedCommandIndex).toBe(1);
  });

  it('passes when sequential commands stay in bounds', () => {
    const commands: EditCommand[] = [
      { type: 'insertAt', position: 11, text: '!' },
      { type: 'replaceRange', start: 0, end: 5, newText: 'hi' },
    ];
    expect(validateCommands(text, commands)).toEqual({ valid: true });
  });
});

describe('edit size limits', () => {
  const stub = 'class Solution:\n    def isPalindrome(self, s: str) -> bool:\n        pass\n';

  it('rejects a command that writes a whole function in one go', () => {
    // Given the edit a "write a two-pointer solution" request produces:
    // the entire body, arriving at once
    const body = [
      '        left, right = 0, len(s) - 1',
      '        while left < right:',
      '            while left < right and not s[left].isalnum():',
      '                left += 1',
      '            if s[left].lower() != s[right].lower():',
      '                return False',
      '            left += 1',
      '            right -= 1',
      '        return True',
    ].join('\n');
    const commands: EditCommand[] = [
      { type: 'replaceRange', start: stub.indexOf('        pass'), end: stub.length, newText: body },
    ];

    // When it is validated
    const result = validateCommands(stub, commands);

    // Then it is refused, and the reason tells the speaker what to do instead
    expect(result.valid).toBe(false);
    expect(result.reason).toContain('smaller steps');
  });

  it('accepts one incremental step', () => {
    // Given the same intent said as its first step only
    const commands: EditCommand[] = [
      {
        type: 'replaceRange',
        start: stub.indexOf('        pass'),
        end: stub.length,
        newText: '        left, right = 0, len(s) - 1\n',
      },
    ];

    // When it is validated
    const result = validateCommands(stub, commands);

    // Then it lands: this is the size voice editing is for
    expect(result.valid).toBe(true);
  });
});

describe('editSizeLimitsFor', () => {
  it('holds a whole-solution ask to the size of a first step', () => {
    // Given the one-breath asks the voice runbook starts every problem with
    const asks = [
      'Write a two-pointer solution that checks whether the string is a palindrome.',
      'Write a hash map solution for two sum.',
      'Write the optimal two pointer solution.',
    ];

    // When the limits for each are read
    // Then each gets the size of a first step
    for (const ask of asks) {
      expect(editSizeLimitsFor(ask)).toEqual({ maxInsertedLines: 3, maxInsertedLinesTotal: 3 });
    }
  });

  it('leaves a single step alone', () => {
    // Given the steps of the iterative solve
    // Then the ordinary limits apply
    expect(editSizeLimitsFor('Add a while loop that runs while left is less than right, with a pass body.')).toBe(DEFAULT_EDIT_SIZE_LIMITS);
    expect(editSizeLimitsFor('Rename the variable left to low.')).toBe(DEFAULT_EDIT_SIZE_LIMITS);
    expect(editSizeLimitsFor(null)).toBe(DEFAULT_EDIT_SIZE_LIMITS);
  });
});
