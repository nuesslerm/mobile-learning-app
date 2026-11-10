import { describe, expect, it } from 'vitest';

import { TAMIL_CHARACTERS } from '../lessons';
import { TRACKS, getTrack, getWritingSubject, splitWord, subjectPosition } from '../tracks';

describe('splitWord', () => {
  it('Given a word with vowel signs and a virama, When split, Then each mark stays with its consonant', () => {
    expect(splitWord('அம்மா')).toEqual(['அ', 'ம்', 'மா']);
    expect(splitWord('கொடு')).toEqual(['கொ', 'டு']);
    expect(splitWord('ஈ')).toEqual(['ஈ']);
  });
});

describe('the tracks', () => {
  it('Given the course, When the tracks are built, Then letters and words are both there', () => {
    const kinds = new Set(TRACKS.map((track) => track.kind));
    expect(kinds).toEqual(new Set(['letters', 'words']));
    expect(getTrack('vowels')?.subjects).toHaveLength(12);
    expect(getTrack('consonants')?.subjects).toHaveLength(18);
    expect(getTrack('combinations')?.subjects).toHaveLength(216);
  });

  it('Given every letter subject, When looked up, Then it has a template, steps and its own id', () => {
    for (const track of TRACKS.filter((t) => t.kind === 'letters')) {
      for (const subject of track.subjects) {
        expect(subject.glyph.length, subject.id).toBeGreaterThan(0);
        expect(subject.steps.length, subject.id).toBeGreaterThan(0);
        expect(subject.characterIds, subject.id).toEqual([subject.id]);
        expect(getWritingSubject(subject.id)?.id).toBe(subject.id);
      }
    }
  });

  it('Given a word, When its subject is built, Then its letters are letters the course knows', () => {
    const words = TRACKS.filter((t) => t.kind === 'words').flatMap((t) => t.subjects);
    expect(words.length).toBeGreaterThan(20);
    const known = new Set(TAMIL_CHARACTERS.map((c) => c.id));
    for (const word of words) {
      expect(word.kind).toBe('word');
      expect(word.steps).toHaveLength(splitWord(word.glyph).length);
      for (const id of word.characterIds) expect(known.has(id), `${word.glyph}: ${id}`).toBe(true);
    }
  });

  it('Given a word from a vowel card, When looked up, Then it is the card\'s own example', () => {
    const amma = getWritingSubject('word-uyir-a');
    expect(amma?.glyph).toBe('அம்மா');
    expect(amma?.label).toBe('mother');
    expect(amma?.characterIds).toEqual(['uyir-a', 'mei-m', 'uyirmei-m-aa']);
  });

  it('Given a subject, When its position is asked, Then the next one is its track neighbour', () => {
    const pos = subjectPosition('uyir-a');
    expect(pos?.track.id).toBe('vowels');
    expect(pos?.track.subjects[(pos?.index ?? 0) + 1]?.id).toBe('uyir-aa');
    expect(subjectPosition('nope')).toBeUndefined();
  });
});
