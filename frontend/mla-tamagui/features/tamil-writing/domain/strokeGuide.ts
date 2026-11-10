import { GENERATED_GLYPHS, type GeneratedGlyph } from './glyphs.generated';
import { describeStroke } from './strokeHints';
import type { Point } from './strokes';

/**
 * Stroke-by-stroke guidance, taken from the letter itself.
 *
 * The first version of this file held paths drawn by hand. They were wrong --
 * the reference for அ was a hexagonal C -- and every test passed anyway,
 * because the validator marked a learner's stroke against those same paths.
 * Nothing ever compared the reference to the letter, so nothing could.
 *
 * Now the geometry comes from the bundled font: `glyphs.generated.ts` carries
 * each letter's outline and the centre line found by thinning it, and
 * `scripts/extract-tamil-glyphs.ts` refuses to emit a letter whose strokes do
 * not stay inside the ink and cover it. The pad prints the same outline, so
 * what is shown, what is traced and what is marked are one geometry.
 *
 * Coordinates are fractions of the pad's box, y downward.
 *
 * Only the twelve உயிர் letters are extracted today. A letter with no entry
 * falls back to free tracing over the font's own glyph rather than being
 * scored against a shape nobody has measured -- see `hasStrokeGuide`.
 */

export type ReferenceStroke = {
  /** The path, in pad fractions. At least two points. */
  points: readonly Point[];
  /** What the hand does, in one line, derived from the path. */
  hint: string;
};

export type StrokeGuide = {
  characterId: string;
  glyph: string;
  strokes: readonly ReferenceStroke[];
  /** The printed letter, for the pad to fill. */
  contours: GeneratedGlyph['contours'];
};

const GUIDES: readonly StrokeGuide[] = GENERATED_GLYPHS.map((glyph) => ({
  characterId: glyph.characterId,
  glyph: glyph.glyph,
  contours: glyph.contours,
  strokes: glyph.strokes.map((points) => ({
    points,
    hint: describeStroke(points),
  })),
}));

const GUIDES_BY_ID = new Map<string, StrokeGuide>(
  GUIDES.map((guide) => [guide.characterId, guide])
);

export function getStrokeGuide(characterId: string): StrokeGuide | undefined {
  return GUIDES_BY_ID.get(characterId);
}

export function hasStrokeGuide(characterId: string): boolean {
  return GUIDES_BY_ID.has(characterId);
}

/** Every letter with measured guidance, for coverage tests and progress copy. */
export function guidedCharacterIds(): string[] {
  return GUIDES.map((guide) => guide.characterId);
}
