import { describe, expect, it } from 'vitest';

import { CURSOR_ACTIONS, caretValueText, motionOfAction } from '../cursorActions';
import { moveBy } from '../cursorMotion';

describe('motionOfAction', () => {
  it.each([
    ['increment', { unit: 'char', count: 1 }],
    ['decrement', { unit: 'char', count: -1 }],
    ['wordRight', { unit: 'word', count: 1 }],
    ['wordLeft', { unit: 'word', count: -1 }],
    ['lineDown', { unit: 'line', count: 1 }],
    ['lineUp', { unit: 'line', count: -1 }],
  ])('given the action %s, when it is mapped, then it is that one motion', (name, motion) => {
    expect(motionOfAction(name)).toEqual(motion);
  });

  it.each(['activate', 'longpress', 'magicTap', 'escape', 'Increment', '', 'constructor', 'toString', '__proto__'])(
    'given the action %j, which the handle does not own, when it is mapped, then it is null',
    (name) => {
      expect(motionOfAction(name)).toBeNull();
    }
  );

  it('given the actions the handle offers, when they are listed, then each has a motion and a label of its own, and increment and decrement come first', () => {
    const names = CURSOR_ACTIONS.map(({ name }) => name);

    expect(names).toEqual(['increment', 'decrement', 'wordRight', 'wordLeft', 'lineDown', 'lineUp']);
    expect(new Set(names).size).toBe(names.length);
    expect(new Set(CURSOR_ACTIONS.map(({ label }) => label)).size).toBe(CURSOR_ACTIONS.length);
    for (const { name, label } of CURSOR_ACTIONS) {
      expect(label.length).toBeGreaterThan(0);
      expect(motionOfAction(name)).not.toBeNull();
    }
  });

  it('given a caret in the middle of some code, when each action is applied through the shared motion code, then it moves one character, word or line the way its name says', () => {
    const text = 'def f(a):\n    return a';
    const from = 16;
    const moved = (name: string) => moveBy(text, from, motionOfAction(name)!);

    expect(moved('increment')).toBe(from + 1);
    expect(moved('decrement')).toBe(from - 1);
    expect(moved('wordRight')).toBeGreaterThan(from);
    expect(moved('wordLeft')).toBeLessThan(from);
    expect(moved('lineUp')).toBeLessThan(text.indexOf('\n') + 1);
    expect(moved('lineDown')).toBe(text.length);
  });
});

describe('caretValueText', () => {
  it.each([
    ['the start of a text', 'abc\ndef', { start: 0, end: 0 }, 'Line 1, column 1'],
    ['the middle of the second line', 'abc\ndef', { start: 5, end: 5 }, 'Line 2, column 2'],
    ['the end of a text', 'abc\ndef', { start: 7, end: 7 }, 'Line 2, column 4'],
    ['the end of a line, before its break', 'abc\ndef', { start: 3, end: 3 }, 'Line 1, column 4'],
    ['an empty text', '', { start: 0, end: 0 }, 'Line 1, column 1'],
    ['after an emoji, counted as one character', 'a😀b', { start: 3, end: 3 }, 'Line 1, column 3'],
  ])('given a caret at %s, when the value is read out, then it says the line and column', (_name, text, selection, expected) => {
    expect(caretValueText(text, selection)).toBe(expected);
  });

  it.each([
    ['within one line', 'abc\ndef', { start: 1, end: 3 }, 'Selected from line 1, column 2 to line 1, column 4'],
    ['across a line break', 'abc\ndef', { start: 1, end: 6 }, 'Selected from line 1, column 2 to line 2, column 3'],
    ['the whole text', 'abc\ndef', { start: 0, end: 7 }, 'Selected from line 1, column 1 to line 2, column 4'],
  ])('given a selection %s, when the value is read out, then it says where the selection runs from and to', (_name, text, selection, expected) => {
    expect(caretValueText(text, selection)).toBe(expected);
  });
});
