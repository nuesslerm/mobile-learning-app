import { describe, expect, it } from 'vitest';

import {
  MASTERY_MASTERED,
  clampMastery,
  isMastered,
  lessonProgress,
  nextCharacterIndex,
  nextMastery,
} from '../mastery';

describe('nextMastery', () => {
  it('Given a new letter, When graded got-it, Then it climbs one rung', () => {
    expect(nextMastery(0, 'got-it')).toBe(1);
  });

  it('Given a letter one rung below mastered, When graded got-it, Then it reaches mastered and stops there', () => {
    expect(nextMastery(MASTERY_MASTERED - 1, 'got-it')).toBe(MASTERY_MASTERED);
    expect(nextMastery(MASTERY_MASTERED, 'got-it')).toBe(MASTERY_MASTERED);
  });

  it('Given a mastered letter, When graded again, Then it drops exactly one rung rather than resetting', () => {
    expect(nextMastery(MASTERY_MASTERED, 'again')).toBe(MASTERY_MASTERED - 1);
  });

  it('Given a letter at zero, When graded again, Then it stays at zero', () => {
    expect(nextMastery(0, 'again')).toBe(0);
  });
});

describe('clampMastery', () => {
  it('Given a value outside the ladder, When clamped, Then it lands on the nearest rung', () => {
    expect(clampMastery(-4)).toBe(0);
    expect(clampMastery(99)).toBe(MASTERY_MASTERED);
  });

  it('Given a non-finite value from corrupt storage, When clamped, Then it reads as zero', () => {
    expect(clampMastery(Number.NaN)).toBe(0);
  });
});

describe('lessonProgress', () => {
  it('Given a lesson nobody has practised, When progress is computed, Then everything is zero', () => {
    const progress = lessonProgress(['a', 'b', 'c'], {});
    expect(progress).toEqual({ total: 3, started: 0, mastered: 0, ratio: 0 });
  });

  it('Given a lesson with one mastered and one part-learned letter, When progress is computed, Then started, mastered and ratio all reflect it', () => {
    const progress = lessonProgress(['a', 'b'], { a: MASTERY_MASTERED, b: 1 });
    expect(progress.started).toBe(2);
    expect(progress.mastered).toBe(1);
    expect(progress.ratio).toBeCloseTo((MASTERY_MASTERED + 1) / (2 * MASTERY_MASTERED));
  });

  it('Given an empty lesson, When progress is computed, Then the ratio is zero rather than NaN', () => {
    expect(lessonProgress([], {}).ratio).toBe(0);
  });
});

describe('isMastered', () => {
  it('Given a letter at the top rung, When asked, Then it reads as mastered', () => {
    expect(isMastered(MASTERY_MASTERED)).toBe(true);
    expect(isMastered(MASTERY_MASTERED - 1)).toBe(false);
  });
});

describe('nextCharacterIndex', () => {
  const ids = ['a', 'b', 'c'];

  it('Given equal mastery everywhere, When picking the next letter, Then it advances in lesson order', () => {
    expect(nextCharacterIndex(ids, {}, 0)).toBe(1);
    expect(nextCharacterIndex(ids, {}, 2)).toBe(0);
  });

  it('Given one weak letter later in the lesson, When picking the next letter, Then the weak one wins over lesson order', () => {
    expect(nextCharacterIndex(ids, { a: 0, b: 3, c: 1 }, 0)).toBe(2);
  });

  it('Given the current letter is the weakest, When picking the next letter, Then it is never shown twice in a row', () => {
    expect(nextCharacterIndex(ids, { a: 0, b: 3, c: 3 }, 0)).not.toBe(0);
  });

  it('Given a one-letter lesson, When picking the next letter, Then it stays on that letter', () => {
    expect(nextCharacterIndex(['only'], {}, 0)).toBe(0);
  });
});
