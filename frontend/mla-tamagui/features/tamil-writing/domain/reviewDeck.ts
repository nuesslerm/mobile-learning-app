/**
 * Two piles, one queue: the flashcard side of the course.
 *
 * A card is a letter to write from memory. Answering it puts it in the pile it
 * earned -- got it, or not yet -- and drops it back into the queue: a missed
 * card comes back after a couple of others, a known one after a dozen, so the
 * letters that need work are the letters that keep appearing. Nothing is ever
 * discarded, so a session can run as long as the learner wants.
 *
 * Pure and synchronous on purpose: the deck is the thing worth testing, and
 * the screen only has to render it. The rungs it starts from come from
 * `mastery.ts`, which is what persists -- the deck itself is a session.
 */
import { MASTERY_MIN, clampMastery, type PracticeOutcome } from './mastery';

/** How far back a missed card goes. Close enough to still be fresh. */
export const AGAIN_GAP = 3;
/** How far back a card the learner got goes. */
export const KNOWN_GAP = 12;

export type ReviewDeck = {
  /** Cards in the order they will be shown; the first is the current one. */
  readonly queue: readonly string[];
  /** Ids the learner has got right at least once, most recent last. */
  readonly known: readonly string[];
  /** Ids the learner has missed and not since got right. */
  readonly learning: readonly string[];
  /** Cards answered this session, including repeats. */
  readonly answered: number;
};

export type ReviewCounts = {
  known: number;
  learning: number;
  unseen: number;
  answered: number;
};

/**
 * Weakest letters first, so a session opens on what the learner has not got
 * rather than on whatever happens to be first in the alphabet.
 */
export function createDeck(
  characterIds: readonly string[],
  mastery: Readonly<Record<string, number>> = {}
): ReviewDeck {
  const queue = [...characterIds].sort((a, b) => {
    const left = clampMastery(mastery[a] ?? MASTERY_MIN);
    const right = clampMastery(mastery[b] ?? MASTERY_MIN);
    if (left !== right) return left - right;
    return characterIds.indexOf(a) - characterIds.indexOf(b);
  });
  return { queue, known: [], learning: [], answered: 0 };
}

export function currentCard(deck: ReviewDeck): string | undefined {
  return deck.queue[0];
}

/** The card after this one, for a peek at what is coming. */
export function nextCard(deck: ReviewDeck): string | undefined {
  return deck.queue[1];
}

export function answer(deck: ReviewDeck, outcome: PracticeOutcome): ReviewDeck {
  const card = currentCard(deck);
  if (!card) return deck;

  const rest = deck.queue.slice(1);
  const gap = outcome === 'got-it' ? KNOWN_GAP : AGAIN_GAP;
  // Past the end of a short queue the card simply goes last: with five cards
  // left there is no twelfth place to put it.
  const at = Math.min(gap, rest.length);
  const queue = [...rest.slice(0, at), card, ...rest.slice(at)];

  const known = deck.known.filter((id) => id !== card);
  const learning = deck.learning.filter((id) => id !== card);

  return {
    queue,
    known: outcome === 'got-it' ? [...known, card] : known,
    learning: outcome === 'got-it' ? learning : [...learning, card],
    answered: deck.answered + 1,
  };
}

export function counts(deck: ReviewDeck): ReviewCounts {
  const seen = deck.known.length + deck.learning.length;
  return {
    known: deck.known.length,
    learning: deck.learning.length,
    unseen: Math.max(deck.queue.length - seen, 0),
    answered: deck.answered,
  };
}
