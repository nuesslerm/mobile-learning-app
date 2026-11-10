/**
 * How a letter moves between "new" and "mastered".
 *
 * A Leitner-style ladder rather than a score: after tracing, the learner says
 * whether it came out right, and the letter moves one rung. Deliberately not
 * spaced repetition with due dates -- writing practice is bounded (47 letters)
 * and a due-date queue would mostly show an empty screen.
 */

export const MASTERY_MIN = 0;
export const MASTERY_MASTERED = 3;

export type PracticeOutcome = 'again' | 'got-it';

export function nextMastery(level: number, outcome: PracticeOutcome): number {
  const current = clampMastery(level);
  if (outcome === 'got-it') return Math.min(current + 1, MASTERY_MASTERED);
  // One rung back, never all the way to zero: a single shaky attempt should not
  // erase three good ones.
  return Math.max(current - 1, MASTERY_MIN);
}

export function clampMastery(level: number): number {
  if (!Number.isFinite(level)) return MASTERY_MIN;
  return Math.min(Math.max(Math.round(level), MASTERY_MIN), MASTERY_MASTERED);
}

export function isMastered(level: number): boolean {
  return clampMastery(level) >= MASTERY_MASTERED;
}

export type LessonProgress = {
  total: number;
  /** Letters with any mastery above zero. */
  started: number;
  mastered: number;
  /** 0..1, averaged over every letter's rung. Drives the progress bar. */
  ratio: number;
};

export function lessonProgress(
  characterIds: readonly string[],
  mastery: Readonly<Record<string, number>>
): LessonProgress {
  const total = characterIds.length;
  if (total === 0) return { total: 0, started: 0, mastered: 0, ratio: 0 };

  let started = 0;
  let mastered = 0;
  let rungs = 0;

  for (const id of characterIds) {
    const level = clampMastery(mastery[id] ?? MASTERY_MIN);
    if (level > MASTERY_MIN) started += 1;
    if (level >= MASTERY_MASTERED) mastered += 1;
    rungs += level;
  }

  return {
    total,
    started,
    mastered,
    ratio: rungs / (total * MASTERY_MASTERED),
  };
}

/**
 * Which letter to show next: the weakest one that is not the one just
 * practised, falling back to the next in lesson order. Keeps a session moving
 * toward the letters that need it without ever repeating the same glyph twice
 * in a row.
 */
export function nextCharacterIndex(
  characterIds: readonly string[],
  mastery: Readonly<Record<string, number>>,
  currentIndex: number
): number {
  const total = characterIds.length;
  if (total === 0) return 0;
  if (total === 1) return 0;

  let bestIndex = -1;
  let bestLevel = Number.POSITIVE_INFINITY;

  // Walk from just after the current letter so equal-mastery ties resolve in
  // lesson order rather than always landing on the first letter.
  for (let step = 1; step <= total; step += 1) {
    const index = (currentIndex + step) % total;
    if (index === currentIndex) continue;
    const level = clampMastery(mastery[characterIds[index]] ?? MASTERY_MIN);
    if (level < bestLevel) {
      bestLevel = level;
      bestIndex = index;
    }
  }

  return bestIndex === -1 ? (currentIndex + 1) % total : bestIndex;
}
