import { describe, expect, it } from 'vitest';

import {
  CONSONANT_ORDER,
  TAMIL_CONSONANTS,
  TAMIL_UYIRMEI,
  VIRAMA,
  VOWEL_SIGNS,
  seriesCharacterIds,
  uyirmeiId,
} from '../uyirmei';

describe('the உயிர்மெய் grid', () => {
  it('Given eighteen consonants and twelve vowels, When generated, Then there are 216 combinations', () => {
    expect(TAMIL_CONSONANTS).toHaveLength(18);
    expect(VOWEL_SIGNS).toHaveLength(12);
    expect(TAMIL_UYIRMEI).toHaveLength(216);
  });

  it('Given a consonant, When its base letter is derived, Then it is the மெய் without its புள்ளி', () => {
    const ka = TAMIL_CONSONANTS[0];
    expect(ka?.meiGlyph).toBe('க்');
    expect(ka?.base).toBe('க');
    expect(ka?.meiGlyph.endsWith(VIRAMA)).toBe(true);
  });

  it('Given every combination, When its glyph is read, Then it is the consonant plus that vowel sign', () => {
    for (const consonant of TAMIL_CONSONANTS) {
      for (const vowel of VOWEL_SIGNS) {
        const id = uyirmeiId(consonant.key, vowel.key);
        const character = TAMIL_UYIRMEI.find((c) => c.id === id);
        expect(character?.glyph).toBe(`${consonant.base}${vowel.sign}`);
      }
    }
  });

  it('Given the க series, When read, Then it spells the twelve forms a learner recites', () => {
    const series = seriesCharacterIds('k').map((id) =>
      TAMIL_UYIRMEI.find((character) => character.id === id)
    );
    expect(series.map((character) => character?.glyph)).toEqual([
      'க', 'கா', 'கி', 'கீ', 'கு', 'கூ', 'கெ', 'கே', 'கை', 'கொ', 'கோ', 'கௌ',
    ]);
  });

  it('Given the inherent vowel, When its form is generated, Then it is the bare letter with no sign at all', () => {
    expect(VOWEL_SIGNS[0]?.sign).toBe('');
    expect(TAMIL_UYIRMEI.find((c) => c.id === 'uyirmei-m-a')?.glyph).toBe('ம');
  });

  it('Given every combination, When its glyph is counted, Then no மெய் virama survived into it', () => {
    for (const character of TAMIL_UYIRMEI) {
      expect(character.glyph.includes(VIRAMA)).toBe(false);
    }
  });

  it('Given the generated grid, When glyphs are collected, Then all 216 are distinct', () => {
    const glyphs = TAMIL_UYIRMEI.map((character) => character.glyph);
    expect(new Set(glyphs).size).toBe(216);
  });

  it('Given the consonant order, When compared to the grid, Then the grid follows the alphabet, not the class grouping', () => {
    expect(CONSONANT_ORDER[0]).toBe('mei-k');
    expect(CONSONANT_ORDER[1]).toBe('mei-ng');
    expect(CONSONANT_ORDER[CONSONANT_ORDER.length - 1]).toBe('mei-n-alveolar');
    expect(TAMIL_CONSONANTS.map((c) => c.meiId)).toEqual([...CONSONANT_ORDER]);
  });

  it('Given a combination, When its transliteration is read, Then it joins the consonant and the vowel', () => {
    const koo = TAMIL_UYIRMEI.find((c) => c.id === 'uyirmei-k-oo');
    expect(koo?.transliteration).toBe('kō');
    expect(koo?.tamilName).toBe('க் + ஓ');
  });

  it('Given the fusing vowels, When their hints are read, Then they warn that the shape changes per letter', () => {
    const u = VOWEL_SIGNS.find((vowel) => vowel.key === 'u');
    expect(u?.hint).toContain('FUSES');
  });

  it('Given a consonant key with no consonant, When its series is requested, Then the ids resolve to nothing rather than throwing', () => {
    const ids = seriesCharacterIds('not-a-consonant');
    expect(ids).toHaveLength(12);
    expect(TAMIL_UYIRMEI.filter((c) => ids.includes(c.id))).toEqual([]);
  });
});
