import { describe, expect, it } from 'vitest';

import { columnOffset, lineColumn, moveBy, stepChars, stepLines, stepWords } from '../cursorMotion';

const insidePair = (text: string, pos: number) =>
  pos > 0 && pos < text.length && text.charCodeAt(pos - 1) >= 0xd800 && text.charCodeAt(pos - 1) <= 0xdbff && text.charCodeAt(pos) >= 0xdc00 && text.charCodeAt(pos) <= 0xdfff;

describe('stepChars', () => {
  it.each([
    ['one right', 'abc', 1, 1, 2],
    ['one left', 'abc', 2, -1, 1],
    ['three right', 'abcdef', 1, 3, 4],
    ['past the end', 'abc', 2, 9, 3],
    ['past the start', 'abc', 1, -9, 0],
    ['no step', 'abc', 1, 0, 1],
    ['over a pair right', 'a😀b', 1, 1, 3],
    ['over a pair left', 'a😀b', 3, -1, 1],
    ['over two pairs', '😀😀', 0, 2, 4],
    ['a position inside a pair, right', 'a😀b', 2, 1, 3],
    ['a position inside a pair, left', 'a😀b', 2, -1, 0],
    ['a line break is one character', 'a\nb', 1, 1, 2],
    ['an empty text', '', 0, 5, 0],
  ])('given %s, when the caret is stepped, then it lands on a character boundary', (_name, text, pos, count, expected) => {
    expect(stepChars(text, pos, count)).toBe(expected);
  });
});

describe('stepWords', () => {
  const code = 'foo.bar(baz)\n    qux';
  const stops = [0, 3, 4, 7, 8, 11, 12, 17, 20];

  it('given code with punctuation, a break and an indent, when it is stepped forward one word at a time, then it stops at each token start, once at a break, and skips the indent', () => {
    const visited = [0];
    for (let at = 0; at < code.length; ) {
      at = stepWords(code, at, 1);
      visited.push(at);
    }
    expect(visited).toEqual(stops);
  });

  it('given the same code, when it is stepped backward one word at a time, then it visits the same stops in reverse', () => {
    const visited = [code.length];
    for (let at = code.length; at > 0; ) {
      at = stepWords(code, at, -1);
      visited.push(at);
    }
    expect(visited).toEqual([...stops].reverse());
  });

  it.each([
    ['inside a word, forward', 'foo bar', 1, 1, 4],
    ['inside a word, backward', 'foo bar', 5, -1, 4],
    ['at a word start, backward', 'foo bar', 4, -1, 0],
    ['two at once', 'foo bar baz', 0, 2, 8],
    ['past the end', 'foo', 0, 5, 3],
    ['past the start', 'foo bar', 5, -9, 0],
    ['a run of punctuation is one stop', 'a)) b', 1, 1, 4],
    ['blank lines are one stop each', 'a\n\n\nb', 1, 1, 2],
    ['the indent of the first line, backward', '    x', 4, -1, 0],
    ['an emoji run is one stop', 'a 😀😀 b', 2, 1, 7],
    ['letters beyond ASCII are one word', 'naïve café', 0, 1, 6],
    ['digits and underscores stay in the word', 'a_1 b', 0, 1, 4],
    ['trailing spaces, then the line end', 'ab   \ncd', 0, 1, 5],
  ])('given %s, when the caret is stepped by words, then it lands on the stop', (_name, text, pos, count, expected) => {
    expect(stepWords(text, pos, count)).toBe(expected);
  });

  it('given a position inside a pair, when it is stepped, then the step starts before the pair', () => {
    expect(stepWords('a 😀 b', 3, 1)).toBe(5);
    expect(stepWords('a 😀 b', 3, -1)).toBe(0);
  });
});

describe('stepLines', () => {
  const text = 'abcdefghij\nab\nabcdef\n\nxyz';
  // line starts: 0, 11, 14, 21, 22

  it('given a caret at column 8, when it moves down, then it takes the goal column clamped to the shorter line', () => {
    expect(stepLines(text, 8, 1)).toBe(13);
  });

  it('given the goal column kept from the grab, when the caret passes a short line, then it comes back to the goal column', () => {
    expect(stepLines(text, 13, 1, 8)).toBe(20);
    expect(stepLines(text, 8, 2)).toBe(20);
  });

  it('given an empty line in the way, when it moves down, then it lands at the empty line and the next line takes the goal column clamped', () => {
    expect(stepLines(text, 8, 3)).toBe(21);
    expect(stepLines(text, 8, 4)).toBe(25);
  });

  it('given a caret on the first line, when it moves up, then it goes to the start of the text', () => {
    expect(stepLines(text, 4, -1)).toBe(0);
    expect(stepLines(text, 4, -9)).toBe(0);
  });

  it('given a caret on the last line, when it moves down, then it goes to the end of the text', () => {
    expect(stepLines(text, 23, 1)).toBe(text.length);
    expect(stepLines(text, 23, 9)).toBe(text.length);
  });

  it('given a caret on the second line, when it moves up, then it lands on the first line at the goal column, not at the start', () => {
    expect(stepLines(text, 13, -1, 8)).toBe(8);
    expect(stepLines(text, 12, -1)).toBe(1);
  });

  it('given no step, when it moves, then the caret stays', () => {
    expect(stepLines(text, 8, 0)).toBe(8);
  });

  it('given a goal column that falls inside a pair, when it moves, then it lands before the pair', () => {
    expect(stepLines('abc\na😀b', 2, 1, 2)).toBe(5);
  });

  it('given a text that starts with a break, when the caret at 0 moves down, then it goes to the second line', () => {
    expect(stepLines('\nab', 0, 1)).toBe(1);
  });
});

describe('lineColumn and columnOffset', () => {
  it.each([
    ['the start', 'ab\ncd', 0, { line: 1, column: 1 }],
    ['the end of the first line', 'ab\ncd', 2, { line: 1, column: 3 }],
    ['the start of the second line', 'ab\ncd', 3, { line: 2, column: 1 }],
    ['the end of the text', 'ab\ncd', 5, { line: 2, column: 3 }],
    ['after an emoji, counted in characters', 'a😀b', 3, { line: 1, column: 3 }],
    ['an empty line', 'a\n\nb', 2, { line: 2, column: 1 }],
    ['a text that starts with a break', '\nab', 0, { line: 1, column: 1 }],
  ])('given %s, when the line and column are read, then they are the ones a person counts from 1', (_name, text, pos, expected) => {
    expect(lineColumn(text, pos)).toEqual(expected);
  });

  it('given a position, when the column offset is read, then it is the distance from the line start in UTF-16 units', () => {
    expect(columnOffset('ab\ncd', 4)).toBe(1);
    expect(columnOffset('a😀b', 3)).toBe(3);
    expect(columnOffset('\nab', 0)).toBe(0);
  });
});

describe('moveBy', () => {
  it('given each unit, when the caret is moved, then it is the same as the step of that unit', () => {
    const text = 'foo bar\nbaz qux';
    expect(moveBy(text, 1, { unit: 'char', count: 2 })).toBe(stepChars(text, 1, 2));
    expect(moveBy(text, 1, { unit: 'word', count: 2 })).toBe(stepWords(text, 1, 2));
    expect(moveBy(text, 1, { unit: 'line', count: 1 })).toBe(stepLines(text, 1, 1));
    expect(moveBy(text, 12, { unit: 'line', count: -1 }, 1)).toBe(stepLines(text, 12, -1, 1));
  });
});

describe('the steps over random text', () => {
  // mulberry32 with a fixed seed: the same texts every run, no clock
  const generator = (seed: number) => () => {
    let t = (seed += 0x6d2b79f5);
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const alphabet = ['a', 'b', 'Z', '_', '7', ' ', '    ', '\t', '\n', '\n', '.', '(', ')', ':', '😀', '𝒜', 'é', '日'];
  const texts = (count: number) => {
    const next = generator(86);
    return Array.from({ length: count }, () => Array.from({ length: Math.floor(next() * 40) }, () => alphabet[Math.floor(next() * alphabet.length)]).join(''));
  };

  it('given random texts, when the caret is stepped by words from one end to the other, then every step is strictly monotone and never inside a surrogate pair', () => {
    for (const text of texts(300)) {
      let forward = 0;
      while (forward < text.length) {
        const next = stepWords(text, forward, 1);
        expect(next).toBeGreaterThan(forward);
        expect(insidePair(text, next)).toBe(false);
        forward = next;
      }
      let backward = text.length;
      while (backward > 0) {
        const next = stepWords(text, backward, -1);
        expect(next).toBeLessThan(backward);
        expect(insidePair(text, next)).toBe(false);
        backward = next;
      }
    }
  });

  it('given random texts, when the caret is stepped by characters or lines, then it never lands inside a surrogate pair and stays in the text', () => {
    for (const text of texts(300)) {
      for (let pos = 0; pos <= text.length; pos += 1) {
        for (const count of [-2, -1, 1, 2]) {
          for (const next of [stepChars(text, pos, count), stepLines(text, pos, count)]) {
            expect(insidePair(text, next)).toBe(false);
            expect(next).toBeGreaterThanOrEqual(0);
            expect(next).toBeLessThanOrEqual(text.length);
          }
        }
      }
    }
  });
});
