import { describe, expect, it } from 'vitest';

import { GENERATED_GLYPHS } from '../glyphs.generated';
import { getStrokeGuide } from '../strokeGuide';
import { VOWEL_SIGN_STEPS, WRITING_STEPS, writingSteps } from '../writingSteps';
import { VOWEL_SIGNS } from '../uyirmei';

describe('the written instructions', () => {
  it('Given the letters that stand on their own, When looked up, Then each has its steps', () => {
    // 12 uyir + ஃ + the 18 consonants in their bare form.
    expect(Object.keys(WRITING_STEPS)).toHaveLength(31);
    for (const [id, entry] of Object.entries(WRITING_STEPS)) {
      expect(entry.steps.length, id).toBeGreaterThan(0);
      for (const step of entry.steps) {
        expect(step.length, `${id} step`).toBeGreaterThan(10);
      }
    }
  });

  it('Given a letter, When its steps and its strokes are counted, Then they agree', () => {
    // The instructions and the geometry come from the same description, so a
    // letter written in two strokes must be taught in two.
    for (const [id, entry] of Object.entries(WRITING_STEPS)) {
      const guide = getStrokeGuide(id);
      if (!guide) continue;
      expect(entry.steps.length, `${id}: steps vs strokes`).toBe(guide.strokes.length);
    }
  });

  it('Given every guided base letter, When its instructions are looked up, Then none is missing', () => {
    const base = GENERATED_GLYPHS.filter(
      (glyph) => !glyph.characterId.startsWith('uyirmei-') || glyph.characterId.endsWith('-a')
    );
    for (const glyph of base) {
      expect(writingSteps(glyph.characterId), glyph.characterId).toBeDefined();
    }
  });

  it('Given the vowel signs, When their lines are looked up, Then every sign has one', () => {
    for (const sign of VOWEL_SIGNS) {
      expect(VOWEL_SIGN_STEPS[sign.key], sign.key).toBeDefined();
    }
    // The bare consonant adds nothing, so its line is empty on purpose.
    expect(VOWEL_SIGN_STEPS.a).toBe('');
  });
});
