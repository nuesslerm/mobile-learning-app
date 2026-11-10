import { describe, expect, it } from 'vitest';

import {
  AGAIN_GAP,
  KNOWN_GAP,
  answer,
  counts,
  createDeck,
  currentCard,
  nextCard,
} from '../reviewDeck';

const LETTERS = Array.from({ length: 20 }, (_, i) => `c${i}`);

describe('createDeck', () => {
  it('Given letters with mastery, When the deck is built, Then the weakest come first', () => {
    const deck = createDeck(['a', 'b', 'c'], { a: 3, b: 0, c: 1 });
    expect(deck.queue).toEqual(['b', 'c', 'a']);
  });

  it('Given letters at the same rung, When the deck is built, Then course order decides', () => {
    const deck = createDeck(['a', 'b', 'c'], {});
    expect(deck.queue).toEqual(['a', 'b', 'c']);
  });

  it('Given a fresh deck, When counted, Then nothing is in either pile', () => {
    expect(counts(createDeck(LETTERS))).toEqual({
      known: 0,
      learning: 0,
      unseen: 20,
      answered: 0,
    });
  });
});

describe('answer', () => {
  it('Given a card the learner missed, When answered, Then it comes back within a few cards', () => {
    const deck = answer(createDeck(LETTERS), 'again');
    expect(deck.queue.indexOf('c0')).toBe(AGAIN_GAP);
    expect(deck.learning).toEqual(['c0']);
    expect(deck.known).toEqual([]);
  });

  it('Given a card the learner got, When answered, Then it goes much further back', () => {
    const deck = answer(createDeck(LETTERS), 'got-it');
    expect(deck.queue.indexOf('c0')).toBe(KNOWN_GAP);
    expect(deck.known).toEqual(['c0']);
  });

  it('Given a missed card and a known one, When both come round, Then the missed one is first', () => {
    let deck = createDeck(LETTERS);
    deck = answer(deck, 'got-it'); // c0 goes far back
    deck = answer(deck, 'again'); // c1 comes back soon
    const missedAt = deck.queue.indexOf('c1');
    const knownAt = deck.queue.indexOf('c0');
    expect(missedAt).toBeLessThan(knownAt);
  });

  it('Given a card missed then got, When the piles are read, Then it has moved across', () => {
    let deck = createDeck(['a', 'b', 'c']);
    deck = answer(deck, 'again');
    expect(deck.learning).toEqual(['a']);
    while (currentCard(deck) !== 'a') deck = answer(deck, 'got-it');
    deck = answer(deck, 'got-it');
    expect(deck.learning).toEqual([]);
    expect(deck.known).toContain('a');
  });

  it('Given a deck shorter than the gap, When a card is answered, Then it simply goes last', () => {
    const deck = answer(createDeck(['a', 'b']), 'got-it');
    expect(deck.queue).toEqual(['b', 'a']);
  });

  it('Given a one-card deck, When answered, Then the same card comes round again', () => {
    const deck = answer(createDeck(['a']), 'again');
    expect(currentCard(deck)).toBe('a');
    expect(deck.answered).toBe(1);
  });

  it('Given an empty deck, When answered, Then nothing happens', () => {
    const empty = createDeck([]);
    expect(answer(empty, 'got-it')).toBe(empty);
    expect(currentCard(empty)).toBeUndefined();
    expect(nextCard(empty)).toBeUndefined();
  });

  it('Given a session, When cards are answered, Then every answer is counted, repeats included', () => {
    let deck = createDeck(['a', 'b']);
    for (let i = 0; i < 5; i += 1) deck = answer(deck, 'again');
    expect(counts(deck).answered).toBe(5);
    // Two letters, both missed: the piles hold letters, not answers.
    expect(counts(deck).learning).toBe(2);
  });
});

describe('the deck as a session', () => {
  it('Given a missed letter, When the session runs on, Then it is seen more often than a known one', () => {
    let deck = createDeck(LETTERS);
    const seen: Record<string, number> = {};
    for (let i = 0; i < 60; i += 1) {
      const card = currentCard(deck);
      if (!card) break;
      seen[card] = (seen[card] ?? 0) + 1;
      // c0 never comes; everything else does.
      deck = answer(deck, card === 'c0' ? 'again' : 'got-it');
    }
    const others = Object.entries(seen)
      .filter(([id]) => id !== 'c0')
      .map(([, n]) => n);
    expect(seen.c0).toBeGreaterThan(Math.max(...others));
  });
});
