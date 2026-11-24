import { describe, expect, it } from 'vitest';

import { DEFAULT_TUNING, createCaretGesture, insideSlop, type CaretBase, type Tuning } from '../cursorDrag';
import { columnOffset, stepChars, stepLines, stepWords } from '../cursorMotion';

const { CHAR_PX, CHAR_ZONE, WORD_PX, LINE_PX, SLOP_PX } = DEFAULT_TUNING;

const LONG_LINE = 'alpha beta.gamma(delta, epsilon) zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau';
const caretAt = (text: string, at: number): CaretBase => ({ text, anchor: at, head: at });

// a locked gesture: the first sample is far enough along the axis to lock it
const lockedX = (base: CaretBase, tuning?: Tuning) => {
  const gesture = createCaretGesture(base, tuning);
  gesture.move({ dx: SLOP_PX, dy: 0 });
  return gesture;
};
const lockedY = (base: CaretBase, tuning?: Tuning) => {
  const gesture = createCaretGesture(base, tuning);
  gesture.move({ dx: 0, dy: SLOP_PX });
  return gesture;
};

describe('the slop and the axis lock', () => {
  it('given a finger that has not left the slop, when it moves, then nothing is targeted and no axis is chosen', () => {
    const gesture = createCaretGesture(caretAt(LONG_LINE, 4));

    expect(gesture.move({ dx: 0, dy: 0 })).toBeNull();
    expect(gesture.move({ dx: SLOP_PX / 2, dy: SLOP_PX / 2 })).toBeNull();
    expect(gesture.axis()).toBeNull();
  });

  it('given a displacement, when it is asked whether it is inside the slop, then the distance from the grab decides, in any direction', () => {
    expect(insideSlop({ dx: 0, dy: 0 })).toBe(true);
    expect(insideSlop({ dx: SLOP_PX - 1, dy: 0 })).toBe(true);
    expect(insideSlop({ dx: 0, dy: -SLOP_PX })).toBe(false);
    expect(insideSlop({ dx: SLOP_PX * 0.8, dy: SLOP_PX * 0.8 })).toBe(false);
  });

  it('given the first sample past the slop is mostly sideways, when the finger later wobbles vertically, then the axis stays x and the wobble moves nothing', () => {
    const base = caretAt('one\ntwo\nthree\nfour\nfive', 1);
    const gesture = createCaretGesture(base);

    const first = gesture.move({ dx: 3 * CHAR_PX, dy: 2 });
    const wobbled = gesture.move({ dx: 3 * CHAR_PX, dy: 5 * LINE_PX });

    expect(gesture.axis()).toBe('x');
    expect(wobbled).toEqual(first);
  });

  it('given the first sample past the slop is mostly vertical, when the finger later wobbles sideways, then the axis stays y and the wobble moves nothing', () => {
    const base = caretAt('one\ntwo\nthree\nfour\nfive', 1);
    const gesture = createCaretGesture(base);

    const first = gesture.move({ dx: 2, dy: 2 * LINE_PX });
    const wobbled = gesture.move({ dx: 9 * CHAR_PX, dy: 2 * LINE_PX });

    expect(gesture.axis()).toBe('y');
    expect(wobbled).toEqual(first);
  });

  it('given a sample exactly diagonal, when the axis is chosen, then it is x', () => {
    const gesture = createCaretGesture(caretAt(LONG_LINE, 0));

    gesture.move({ dx: SLOP_PX, dy: SLOP_PX });

    expect(gesture.axis()).toBe('x');
  });

  it('given a locked gesture, when the finger returns inside the slop, then it is the base and not null, because the axis stays chosen', () => {
    const base = caretAt(LONG_LINE, 5);
    const gesture = lockedX(base);

    expect(gesture.move({ dx: 0, dy: 0 })).toEqual({ anchor: 5, head: 5 });
  });
});

describe('sideways travel', () => {
  it.each([1, 2, 5, CHAR_ZONE])('given %i characters of travel inside the character zone, when it moves right, then the caret is that many characters on', (chars) => {
    const base = caretAt(LONG_LINE, 3);

    expect(lockedX(base).move({ dx: chars * CHAR_PX, dy: 0 })?.head).toBe(stepChars(LONG_LINE, 3, chars));
  });

  it('given travel short of a whole character, when it moves, then the caret has not moved', () => {
    expect(lockedX(caretAt(LONG_LINE, 3)).move({ dx: CHAR_PX - 1, dy: 0 })?.head).toBe(3);
  });

  it.each([1, 2, 3])('given %i words of travel past the character zone, when it moves right, then the caret is a word start that many stops beyond the zone', (words) => {
    const base = caretAt(LONG_LINE, 0);
    const beyondZone = stepChars(LONG_LINE, 0, CHAR_ZONE);

    const head = lockedX(base).move({ dx: CHAR_ZONE * CHAR_PX + words * WORD_PX, dy: 0 })?.head as number;

    expect(head).toBe(stepWords(LONG_LINE, beyondZone, words));
    expect(stepWords(LONG_LINE, stepWords(LONG_LINE, head, -1), 1)).toBe(head);
  });

  it('given travel past the character zone but short of a word, when it moves, then the caret stays where the zone ended', () => {
    const head = lockedX(caretAt(LONG_LINE, 0)).move({ dx: CHAR_ZONE * CHAR_PX + WORD_PX - 1, dy: 0 })?.head;

    expect(head).toBe(stepChars(LONG_LINE, 0, CHAR_ZONE));
  });

  it('given leftward travel, when it moves, then it mirrors the rightward steps from the grab', () => {
    const base = caretAt(LONG_LINE, 60);

    expect(lockedX(base).move({ dx: -4 * CHAR_PX, dy: 0 })?.head).toBe(stepChars(LONG_LINE, 60, -4));
    const far = CHAR_ZONE * CHAR_PX + 2 * WORD_PX;
    expect(lockedX(base).move({ dx: -far, dy: 0 })?.head).toBe(stepWords(LONG_LINE, stepChars(LONG_LINE, 60, -CHAR_ZONE), -2));
  });

  it('given travel past the end of the text, when it moves, then the caret stops at the end', () => {
    expect(lockedX(caretAt('abc', 1)).move({ dx: 100 * WORD_PX, dy: 0 })?.head).toBe(3);
    expect(lockedX(caretAt('abc', 1)).move({ dx: -100 * WORD_PX, dy: 0 })?.head).toBe(0);
  });
});

describe('vertical travel', () => {
  const text = Array.from({ length: 40 }, (_, line) => `line${line} ${'x'.repeat(line % 9)}`).join('\n');
  const base = caretAt(text, text.indexOf('line10') + 3);
  const goal = columnOffset(text, base.head);

  it.each([1, 2, 6])('given %i lines of travel down, when it moves, then the caret is that many lines on at the column of the grab', (lines) => {
    expect(lockedY(base).move({ dx: 0, dy: lines * LINE_PX })?.head).toBe(stepLines(text, base.head, lines, goal));
  });

  it.each([7, 9])('given %i lines of travel down, when it moves, then the lines beyond the sixth count three each', (lines) => {
    expect(lockedY(base).move({ dx: 0, dy: lines * LINE_PX })?.head).toBe(stepLines(text, base.head, 6 + 3 * (lines - 6), goal));
  });

  it('given travel up, when it moves, then it goes up the same number of lines', () => {
    expect(lockedY(base).move({ dx: 0, dy: -3 * LINE_PX })?.head).toBe(stepLines(text, base.head, -3, goal));
  });

  it('given a drag through short lines, when it comes to a long line again, then the column of the grab is back, not the short line’s end', () => {
    const sample = 'abcdefghij\nab\nabcdefghij';
    const grab = caretAt(sample, 8);

    const throughShort = lockedY(grab).move({ dx: 0, dy: LINE_PX })?.head;
    const pastShort = lockedY(grab).move({ dx: 0, dy: 2 * LINE_PX })?.head;

    expect(throughShort).toBe(13);
    expect(pastShort).toBe(sample.length - 2);
  });

  it('given travel short of a whole line, when it moves, then the caret has not moved', () => {
    expect(lockedY(base).move({ dx: 0, dy: LINE_PX - 1 })?.head).toBe(base.head);
  });

  it('given travel up from the first line, when it moves, then the caret goes to the start of the text', () => {
    expect(lockedY(caretAt('abc\ndef', 2)).move({ dx: 0, dy: -LINE_PX })?.head).toBe(0);
  });
});

describe('a selection at the grab', () => {
  const text = 'abcdefghij\nabcdefghij';

  it('given a selection, when the finger drags sideways, then the head moves and the anchor stays', () => {
    const target = lockedX({ text, anchor: 2, head: 5 }).move({ dx: 3 * CHAR_PX, dy: 0 });

    expect(target).toEqual({ anchor: 2, head: 8 });
  });

  it('given a selection whose head is before its anchor, when the finger drags vertically, then the head moves a line and the anchor stays', () => {
    const target = lockedY({ text, anchor: 8, head: 3 }).move({ dx: 0, dy: LINE_PX });

    expect(target).toEqual({ anchor: 8, head: 14 });
  });

  it('given a selection, when the head is dragged back across the anchor, then the selection turns over it and the anchor stays', () => {
    expect(lockedX({ text, anchor: 5, head: 6 }).move({ dx: -3 * CHAR_PX, dy: 0 })).toEqual({ anchor: 5, head: 3 });
  });

  it('given a caret, when the finger drags, then the anchor goes with the head', () => {
    expect(lockedX(caretAt(text, 2)).move({ dx: 3 * CHAR_PX, dy: 0 })).toEqual({ anchor: 5, head: 5 });
  });
});

describe('the mapping as a whole', () => {
  it('given any sideways distance, when it grows, then the target never goes back', () => {
    const base = caretAt(LONG_LINE, 20);
    let last = lockedX(base).move({ dx: -300, dy: 0 })?.head as number;
    for (let dx = -299; dx <= 300; dx += 1) {
      const head = lockedX(base).move({ dx, dy: 0 })?.head as number;
      expect(head).toBeGreaterThanOrEqual(last);
      last = head;
    }
  });

  it('given any vertical distance, when it grows, then the target never goes back', () => {
    const text = Array.from({ length: 60 }, (_, line) => `row ${line}`).join('\n');
    const base = caretAt(text, 30);
    let last = lockedY(base).move({ dx: 0, dy: -400 })?.head as number;
    for (let dy = -399; dy <= 1600; dy += 1) {
      const head = lockedY(base).move({ dx: 0, dy })?.head as number;
      expect(head).toBeGreaterThanOrEqual(last);
      last = head;
    }
  });

  it('given a drag that went far and came back to the grab, when the finger is at the origin, then the caret is where it started', () => {
    const base = caretAt(LONG_LINE, 30);
    const gesture = lockedX(base);

    gesture.move({ dx: 200, dy: 0 });
    gesture.move({ dx: -150, dy: 0 });

    expect(gesture.move({ dx: 0, dy: 0 })).toEqual({ anchor: 30, head: 30 });
  });

  it('given a retuned handle, when it moves, then the new numbers set the feel and the old ones do not', () => {
    const tuning: Tuning = { CHAR_PX: 20, CHAR_ZONE: 4, WORD_PX: 50, LINE_PX: 30, SLOP_PX: 4 };
    const gesture = createCaretGesture(caretAt(LONG_LINE, 0), tuning);

    gesture.move({ dx: 4, dy: 0 });

    expect(gesture.move({ dx: 3 * tuning.CHAR_PX, dy: 0 })?.head).toBe(3);
    expect(gesture.move({ dx: 4 * tuning.CHAR_PX + tuning.WORD_PX, dy: 0 })?.head).toBe(stepWords(LONG_LINE, 4, 1));
  });
});
