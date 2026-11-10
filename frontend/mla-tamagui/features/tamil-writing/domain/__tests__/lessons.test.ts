import { describe, expect, it } from 'vitest';

import { TAMIL_BASE_CHARACTERS } from '../characters';
import {
  TAMIL_CHARACTERS,
  TAMIL_LESSONS,
  getCharacter,
  getLesson,
  getLessonCharacters,
  lessonCharacterIdsResolve,
  lessonsBySection,
} from '../lessons';
import { TAMIL_UYIRMEI } from '../uyirmei';

describe('the Tamil course catalogue', () => {
  it('Given the lesson list, When every character id is looked up, Then all of them resolve', () => {
    expect(lessonCharacterIdsResolve()).toBe(true);
  });

  it('Given the whole alphabet, When counted, Then it is the classical 247 plus the four grantha letters', () => {
    expect(TAMIL_BASE_CHARACTERS).toHaveLength(35);
    expect(TAMIL_UYIRMEI).toHaveLength(216);
    expect(TAMIL_CHARACTERS).toHaveLength(251);
  });

  it('Given the character list, When ids are collected, Then none is duplicated', () => {
    const ids = TAMIL_CHARACTERS.map((character) => character.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it('Given the character list, When glyphs are collected, Then none is duplicated', () => {
    const glyphs = TAMIL_CHARACTERS.map((character) => character.glyph);
    expect(new Set(glyphs).size).toBe(glyphs.length);
  });

  it('Given the lesson list, When characters are pooled, Then every character belongs to exactly one lesson', () => {
    const lessonIds = TAMIL_LESSONS.flatMap((lesson) => [...lesson.characterIds]);
    expect(new Set(lessonIds).size).toBe(lessonIds.length);
    expect(new Set(lessonIds)).toEqual(
      new Set(TAMIL_CHARACTERS.map((character) => character.id))
    );
  });

  it('Given the lesson list, When counted, Then it is the six base lessons plus one per consonant', () => {
    expect(TAMIL_LESSONS).toHaveLength(6 + 18);
    expect(
      TAMIL_LESSONS.filter((lesson) => lesson.section === 'uyirmei')
    ).toHaveLength(18);
  });

  it('Given the vowel lessons, When their characters are read, Then they hold the twelve உயிர் letters in order', () => {
    const vowels = [
      ...getLessonCharacters('uyir-1'),
      ...getLessonCharacters('uyir-2'),
    ];
    expect(vowels.map((character) => character.glyph)).toEqual([
      'அ', 'ஆ', 'இ', 'ஈ', 'உ', 'ஊ', 'எ', 'ஏ', 'ஐ', 'ஒ', 'ஓ', 'ஔ',
    ]);
  });

  it('Given every உயிர்மெய் lesson, When its characters are read, Then it holds that consonant and twelve vowels', () => {
    for (const lesson of TAMIL_LESSONS.filter((l) => l.section === 'uyirmei')) {
      const series = getLessonCharacters(lesson.id);
      expect(series).toHaveLength(12);
      expect(series.every((character) => character.kind === 'uyirmei')).toBe(true);
    }
  });

  it('Given an unknown lesson id, When it is looked up, Then it resolves to nothing and yields no characters', () => {
    expect(getLesson('no-such-lesson')).toBeUndefined();
    expect(getLessonCharacters('no-such-lesson')).toEqual([]);
  });

  it('Given a known character id, When it is looked up, Then the letter comes back', () => {
    expect(getCharacter('uyir-a')?.glyph).toBe('அ');
    expect(getCharacter('uyirmei-k-oo')?.glyph).toBe('கோ');
    expect(getCharacter('no-such-character')).toBeUndefined();
  });

  it('Given every character, When its teaching fields are read, Then none is blank', () => {
    for (const character of TAMIL_CHARACTERS) {
      expect(character.transliteration.length).toBeGreaterThan(0);
      expect(character.tamilName.length).toBeGreaterThan(0);
      expect(character.sound.length).toBeGreaterThan(0);
      expect(character.strokeHint.length).toBeGreaterThan(0);
    }
  });
});

describe('lessonsBySection', () => {
  it('Given the course, When grouped, Then the sections come back in course order with every lesson kept', () => {
    const groups = lessonsBySection();
    expect(groups.map((group) => group.section)).toEqual([
      'uyir',
      'mei',
      'other',
      'uyirmei',
    ]);
    expect(groups.flatMap((group) => group.lessons)).toHaveLength(
      TAMIL_LESSONS.length
    );
  });

  it('Given the grouped course, When a section is read, Then its title names the section in Tamil and English', () => {
    const uyirmei = lessonsBySection().find((g) => g.section === 'uyirmei');
    expect(uyirmei?.title).toContain('உயிர்மெய்');
    expect(uyirmei?.title).toContain('Combinations');
    expect(uyirmei?.lessons).toHaveLength(18);
  });
});
