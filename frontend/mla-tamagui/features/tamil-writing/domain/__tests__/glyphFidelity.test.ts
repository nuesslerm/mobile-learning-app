import { describe, expect, it } from 'vitest';

import { GENERATED_GLYPHS } from '../glyphs.generated';
import {
  DEFAULT_RASTER_SIZE,
  inkCoverage,
  isInsideOutline,
  outlineBounds,
  pathOnGlyph,
  rasterizeGlyph,
  uncoveredClusters,
} from '../glyphGeometry';
import { getStrokeGuide, guidedCharacterIds } from '../strokeGuide';
import { assessStroke } from '../strokeValidation';
import type { Point, Stroke } from '../strokes';

/**
 * The test that did not exist, and whose absence is why a hexagonal C shipped
 * as the guide for அ.
 *
 * Everything else in this feature checks the app against its own reference
 * path. These check the reference path against the LETTER.
 */

/** Matches `scripts/extract-tamil-glyphs.ts`; the gate it enforces at build. */
const PEN_RADIUS = 0.07;
const MIN_COVERAGE = 0.84;
const MIN_ON_GLYPH = 0.85;
const CANVAS = { width: 300, height: 300 };

const rasters = new Map(
  GENERATED_GLYPHS.map((glyph) => [
    glyph.characterId,
    rasterizeGlyph({ contours: glyph.contours }, DEFAULT_RASTER_SIZE),
  ])
);

function rasterOf(characterId: string) {
  const raster = rasters.get(characterId);
  if (!raster) throw new Error(`no raster for ${characterId}`);
  return raster;
}

function traced(points: readonly Point[]): Stroke {
  return points.map((point) => ({
    x: point.x * CANVAS.width,
    y: point.y * CANVAS.height,
  }));
}

describe('every guided letter', () => {
  it.each(GENERATED_GLYPHS.map((glyph) => [glyph.characterId, glyph.glyph]))(
    '%s (%s): every stroke stays inside the printed letter',
    (characterId) => {
      const raster = rasterOf(String(characterId));
      const guide = getStrokeGuide(String(characterId));
      expect(guide).toBeDefined();
      for (const [index, stroke] of (guide?.strokes ?? []).entries()) {
        const onGlyph = pathOnGlyph(raster, stroke.points);
        expect(`stroke ${index}: ${(onGlyph * 100).toFixed(1)}%`).toBe(
          `stroke ${index}: ${Math.max(onGlyph * 100, MIN_ON_GLYPH * 100).toFixed(1)}%`
        );
      }
    }
  );

  it.each(GENERATED_GLYPHS.map((glyph) => [glyph.characterId, glyph.glyph]))(
    '%s (%s): the strokes together account for the whole letter',
    (characterId) => {
      const raster = rasterOf(String(characterId));
      const guide = getStrokeGuide(String(characterId));
      const coverage = inkCoverage(
        raster,
        (guide?.strokes ?? []).map((stroke) => stroke.points),
        PEN_RADIUS
      );
      expect(coverage).toBeGreaterThanOrEqual(MIN_COVERAGE);
    }
  );

  it.each(GENERATED_GLYPHS.map((glyph) => [glyph.characterId, glyph.glyph]))(
    '%s (%s): tracing the guide exactly is accepted by the marking',
    (characterId) => {
      const guide = getStrokeGuide(String(characterId));
      for (const stroke of guide?.strokes ?? []) {
        const result = assessStroke(traced(stroke.points), stroke, CANVAS);
        expect(`${String(characterId)}: ${result.reason ?? 'ok'}`).toBe(
          `${String(characterId)}: ok`
        );
      }
    }
  );
});

describe('the fidelity measurements themselves', () => {
  /**
   * The guard on the guard: a metric that cannot fail is not a test. These
   * feed it the shapes that were actually shipped and shows it rejects them.
   */
  it('Given a hand-drawn path of the kind that shipped, When measured against அ, Then it is nowhere near the letter', () => {
    const raster = rasterOf('uyir-a');
    // The original hand-authored first stroke for அ, verbatim.
    const handDrawn = [
      { x: 0.46, y: 0.3 },
      { x: 0.33, y: 0.26 },
      { x: 0.23, y: 0.35 },
      { x: 0.22, y: 0.5 },
      { x: 0.3, y: 0.63 },
      { x: 0.45, y: 0.68 },
      { x: 0.58, y: 0.62 },
    ];
    expect(pathOnGlyph(raster, handDrawn)).toBeLessThan(MIN_ON_GLYPH);
  });

  it('Given a straight line across the pad, When measured, Then most of it is off any letter', () => {
    const raster = rasterOf('uyir-i');
    const diagonal = [
      { x: 0.05, y: 0.05 },
      { x: 0.95, y: 0.95 },
    ];
    expect(pathOnGlyph(raster, diagonal)).toBeLessThan(0.8);
  });

  it('Given a single short stroke, When coverage is measured, Then it does not pass for the whole letter', () => {
    const raster = rasterOf('uyir-ai');
    const stub = [
      { x: 0.45, y: 0.5 },
      { x: 0.55, y: 0.5 },
    ];
    expect(inkCoverage(raster, [stub], PEN_RADIUS)).toBeLessThan(0.3);
  });

  it('Given the strokes of a letter, When the leftovers are looked for, Then nothing sizeable is unaccounted for', () => {
    for (const glyph of GENERATED_GLYPHS) {
      const raster = rasterOf(glyph.characterId);
      const clusters = uncoveredClusters(
        raster,
        glyph.strokes,
        PEN_RADIUS,
        // A detached mark drawn as a two-pixel tick leaves its own ink
        // around it uncovered -- ஈ's bowls and ஔ's tail. A patch bigger than
        // an eighth of the letter is a missing stroke, not a fat mark.
        Math.round(raster.inkCount * 0.125)
      );
      expect(`${glyph.characterId}: ${clusters.length}`).toBe(
        `${glyph.characterId}: 0`
      );
    }
  });
});

describe('the extracted outlines', () => {
  it('Given every letter, When its outline is measured, Then it fills the pad without leaving it', () => {
    for (const glyph of GENERATED_GLYPHS) {
      const bounds = outlineBounds({ contours: glyph.contours });
      expect(bounds.minX).toBeGreaterThanOrEqual(0);
      expect(bounds.minY).toBeGreaterThanOrEqual(0);
      expect(bounds.maxX).toBeLessThanOrEqual(1);
      expect(bounds.maxY).toBeLessThanOrEqual(1);
      // Aspect is preserved, so only the longer side reaches the margin.
      const longest = Math.max(
        bounds.maxX - bounds.minX,
        bounds.maxY - bounds.minY
      );
      expect(longest).toBeGreaterThan(0.75);
    }
  });

  it('Given a point in the middle of a stroke and one well outside, When tested, Then the outline tells them apart', () => {
    const glyph = GENERATED_GLYPHS.find((g) => g.characterId === 'uyir-u');
    if (!glyph) throw new Error('uyir-u is not extracted');
    const outline = { contours: glyph.contours };
    const onStroke = glyph.strokes[0]?.[3];
    if (!onStroke) throw new Error('uyir-u has no stroke samples');
    expect(isInsideOutline(outline, onStroke)).toBe(true);
    expect(isInsideOutline(outline, { x: 0.01, y: 0.99 })).toBe(false);
  });

  it('Given the course, When guided letters are counted, Then every vowel, ஃ and every base consonant is covered', () => {
    // 12 uyir + ஃ + the 18 consonants in their bare (க, not க்) form, the 31
    // letters UPenn animates, plus 97 combinations composed from a consonant's
    // own strokes and the vowel sign as the க series writes it, each measured
    // against the font's own rendering of the combination.
    expect(guidedCharacterIds()).toHaveLength(110);
    const base = guidedCharacterIds().filter(
      (id) => !id.startsWith('uyirmei-') || id.endsWith('-a')
    );
    expect(base).toHaveLength(31);
    expect(GENERATED_GLYPHS.slice(0, 13).map((glyph) => glyph.glyph)).toEqual([
      'அ', 'ஆ', 'இ', 'ஈ', 'உ', 'ஊ', 'எ', 'ஏ', 'ஐ', 'ஒ', 'ஓ', 'ஔ', 'ஃ',
    ]);
  });

  it('Given the base letters, When their strokes are counted, Then each matches how the letter is written', () => {
    // The counts come from a Tamil writer's description of how each letter is
    // formed (COUNTS in extract-upenn-strokes.py), and every letter now hits
    // its own. Each stroke is a trail over the letter's skeleton: it passes
    // through crossings but redraws a line only where the shape leaves no
    // choice, and then the SHORTEST one -- ர's three arms meet at a point, so
    // one of them is always drawn twice however the letter is written.
    const base = GENERATED_GLYPHS.filter(
      (glyph) => !glyph.characterId.startsWith('uyirmei-') || glyph.characterId.endsWith('-a')
    );
    expect(base.map((glyph) => `${glyph.glyph}:${glyph.strokes.length}`)).toEqual([
      'அ:2', 'ஆ:3', 'இ:1', 'ஈ:4', 'உ:1', 'ஊ:2', 'எ:2', 'ஏ:3', 'ஐ:1', 'ஒ:1', 'ஓ:1', 'ஔ:2', 'ஃ:3',
      'க:2', 'ங:2', 'ச:2', 'ஞ:2', 'ட:1', 'ண:1', 'த:2', 'ந:2', 'ப:1', 'ம:2', 'ய:1', 'ர:1', 'ல:1',
      'வ:1', 'ழ:2', 'ள:1', 'ற:1', 'ன:1',
    ]);
  });

  it('Given a combination, When its strokes are counted, Then the vowel sign adds exactly the marks it is written with', () => {
    // A sign on one side of the consonant is one more stroke; ொ, ோ and ௌ put a
    // mark on each side, so they are three.
    const counts = (id: string) =>
      GENERATED_GLYPHS.find((glyph) => glyph.characterId === id)?.strokes.length;
    // A combination carries the consonant's own strokes plus the sign's. ெ is
    // one mark; ா's skeleton needs two trails to cover without doubling back,
    // and it keeps the extra lift rather than drawing a line twice.
    expect(counts('uyirmei-m-a')).toBe(2);
    const ma = counts('uyirmei-m-a') ?? 0;
    expect(counts('uyirmei-m-e')).toBe(ma + 1);
    expect(counts('uyirmei-m-aa')).toBe(ma + 2);
    expect(counts('uyirmei-m-oo')).toBe(ma + 3);
    // ர's combinations follow its own count the same way.
    expect(counts('uyirmei-r-aa')).toBe((counts('uyirmei-r-a') ?? 0) + 2);
  });
});
