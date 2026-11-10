/**
 * Pulls the Tamil letterforms out of the bundled font and turns each into the
 * two things the course needs: the outline the pad prints, and the centre line
 * a hand writes along.
 *
 * Run: `pnpm --filter mla-tamagui run glyphs:extract`
 * Writes: features/tamil-writing/domain/glyphs.generated.ts
 *
 * This exists because hand-drawn stroke paths were wrong and nothing could
 * tell. The validator marked a learner against the same hand-drawn path, so
 * the only thing ever checked was that the app agreed with itself -- and the
 * reference for அ was a hexagonal C. Geometry taken from the font cannot
 * disagree with the printed letter, because it IS the printed letter.
 *
 * The skeleton comes from the same pure modules the tests use
 * (`glyphGeometry.ts`, `skeleton.ts`), so what ships is what was measured.
 */
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  DEFAULT_RASTER_SIZE,
  inkCoverage,
  pathOnGlyph,
  rasterizeGlyph,
  type GlyphContour,
} from '../features/tamil-writing/domain/glyphGeometry';
import { simplify } from '../features/tamil-writing/domain/geometry';
import type { Point } from '../features/tamil-writing/domain/strokes';

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = join(
  HERE,
  '..',
  'features',
  'tamil-writing',
  'domain',
  'glyphs.generated.ts'
);

/**
 * Letters to extract, by the course's character id: the 31 UPenn animates as
 * letters in their own right. The க-series combinations it also animates are
 * NOT here -- compose-uyirmei-strokes.py cuts those animations at the glyph
 * boundaries instead, so கா comes out as க's own two strokes plus the ா, the
 * same way மா does.
 */
const CHARACTERS: readonly (readonly [string, string, string])[] = [
  ['uyir-a', 'அ', 'alet'],
  ['uyir-aa', 'ஆ', 'aalet'],
  ['uyir-i', 'இ', 'ilet'],
  ['uyir-ii', 'ஈ', 'iilet'],
  ['uyir-u', 'உ', 'ulet'],
  ['uyir-uu', 'ஊ', 'uulet'],
  ['uyir-e', 'எ', 'elet'],
  ['uyir-ee', 'ஏ', 'eelet'],
  ['uyir-ai', 'ஐ', 'ailet'],
  ['uyir-o', 'ஒ', 'olet'],
  ['uyir-oo', 'ஓ', 'oolet'],
  ['uyir-au', 'ஔ', 'aulet'],
  ['aaytham', 'ஃ', 'ahlet'],
  ['uyirmei-k-a', 'க', 'ka'],
  ['uyirmei-ng-a', 'ங', 'nga'],
  ['uyirmei-c-a', 'ச', 'ca'],
  ['uyirmei-nj-a', 'ஞ', 'nja'],
  ['uyirmei-tt-a', 'ட', 't8a'],
  ['uyirmei-nn-a', 'ண', 'n3a'],
  ['uyirmei-t-a', 'த', 'tha'],
  ['uyirmei-n-dental-a', 'ந', 'nda'],
  ['uyirmei-p-a', 'ப', 'pa'],
  ['uyirmei-m-a', 'ம', 'ma'],
  ['uyirmei-y-a', 'ய', 'ya'],
  ['uyirmei-r-a', 'ர', 'ra'],
  ['uyirmei-l-a', 'ல', 'la'],
  ['uyirmei-v-a', 'வ', 'va'],
  ['uyirmei-zh-a', 'ழ', 'zha'],
  ['uyirmei-ll-a', 'ள', 'l9a'],
  ['uyirmei-rr-a', 'ற', 'r6a'],
  ['uyirmei-n-alveolar-a', 'ன', 'n2a'],
];

/**
 * Stroke order and direction, taken from the University of Pennsylvania Tamil
 * program's own letter-writing animations
 * (ccat.sas.upenn.edu/plc/tamilweb) by
 * `scripts/tamil-strokes/extract-upenn-strokes.py`: each animation draws the
 * letter frame by frame, so the order the ink appears IS the order a hand
 * writes it, and a frame whose ink does not touch what is already drawn is a
 * pen lift.
 *
 * This is the part geometry cannot recover. The shape of a letter says nothing
 * about where the pen started or when it left the paper -- every attempt to
 * infer it from the skeleton produced counts that were plainly wrong (ஐ as
 * eight strokes, or forced to one by drawing chords across the letter).
 */
type UpennGlyph = {
  strokes: number[][][];
  contours: number[][][];
  aspect: number[];
};

const UPENN: Record<string, UpennGlyph> = JSON.parse(
  readFileSync(join(HERE, 'tamil-strokes', 'upenn-strokes.json'), 'utf8')
) as Record<string, UpennGlyph>;

/**
 * The combinations UPenn does not animate, composed by
 * `scripts/tamil-strokes/compose-uyirmei-strokes.py`: the consonant's own
 * strokes and the vowel sign's strokes as the க series writes them, placed
 * where HarfBuzz shapes the real string. The outline it ships with is traced
 * from the font's rendering of the combination rather than from the strokes,
 * so the gates below are measuring the composition against a drawing of the
 * letter it had no part in -- a composed letter whose pieces do not land is
 * dropped rather than shipped.
 */
const COMPOSED: Record<string, UpennGlyph & { glyph: string }> = JSON.parse(
  readFileSync(join(HERE, 'tamil-strokes', 'composed-strokes.json'), 'utf8')
) as Record<string, UpennGlyph & { glyph: string }>;

/** Leaves this much of the pad clear around the letter. */
const MARGIN = 0.1;
/** Outline detail below this is invisible on a phone and costs bundle bytes. */
const OUTLINE_TOLERANCE = 0.002;
/** Tried coarsest first; the first that keeps the stroke on the letter wins. */
const SKELETON_TOLERANCES = [0.012, 0.008, 0.005, 0.003, 0.0015];
/** Half the pen width the strokes stand for, in pad fractions. */
const PEN_RADIUS = 0.07;
/**
 * Below this the strokes are not the letter; the script refuses to ship them.
 * Not 0.95, because a stroke is a centre line and some letters carry marks too
 * small to have one -- ஈ's dots are a couple of pixels once thinned, and a
 * couple of pixels cannot cover the ink around them. ந sits at 84.6%: the foot
 * of its animated letterform is a flourish the centre line does not fill.
 */
const MIN_COVERAGE = 0.84;
/**
 * Every stroke has to be this far inside the ink. 30 of the 31 letters clear
 * 96.7%; ஈ sits at 86% because its two bowls are drawn as short marks whose
 * skeleton is a couple of pixels, and a couple of pixels cannot hug a curve.
 */
const MIN_ON_GLYPH = 0.85;

function round(value: number): number {
  return Math.round(value * 1000) / 1000;
}


type Extracted = {
  id: string;
  character: string;
  contours: GlyphContour[];
  strokes: Point[][];
};

const extracted: Extracted[] = [];

type Built = {
  contours: GlyphContour[];
  strokes: Point[][];
  coverage: number;
  worstOnGlyph: number;
  problem?: string;
};

function build(id: string, character: string, source: UpennGlyph): Built {
  // The printed letter is the animation's own letterform, not the font's.
  // Mixing them was measurably wrong: Noto's உ and the animation's உ differ
  // enough in proportion that the traced stroke sat 34% on the printed glyph.
  // One source for the shape and the strokes, so they cannot disagree.
  const aspect = source.aspect;
  const fit = 1 - MARGIN * 2;
  const offsetX = (1 - (aspect[0] ?? 1) * fit) / 2;
  const offsetY = (1 - (aspect[1] ?? 1) * fit) / 2;
  const place = (point: number[]): Point => ({
    x: round((point[0] ?? 0) * fit + offsetX),
    y: round((point[1] ?? 0) * fit + offsetY),
  });

  const contours = source.contours
    .map((contour) => simplify(contour.map(place), OUTLINE_TOLERANCE))
    .filter((contour) => contour.length >= 3);
  if (contours.length === 0) {
    throw new Error(`${id}: "${character}" produced no outline`);
  }

  const raster = rasterizeGlyph({ contours }, DEFAULT_RASTER_SIZE);

  // Stroke order and direction come from the writing animation, in the same
  // coordinates as the outline above.
  const tidy = (path: Point[]): Point[] => {
    for (const tolerance of SKELETON_TOLERANCES) {
      const candidate = simplify(path, tolerance).map((point) => ({
        x: round(point.x),
        y: round(point.y),
      }));
      if (pathOnGlyph(raster, candidate) >= MIN_ON_GLYPH) return candidate;
    }
    return simplify(path, SKELETON_TOLERANCES[SKELETON_TOLERANCES.length - 1] ?? 0.002).map(
      (point) => ({ x: round(point.x), y: round(point.y) })
    );
  };

  const strokes = source.strokes
    .map((stroke) => tidy(stroke.map(place)))
    .filter((stroke) => stroke.length >= 2);

  const coverage = strokes.length === 0 ? 0 : inkCoverage(raster, strokes, PEN_RADIUS);
  const worstOnGlyph =
    strokes.length === 0
      ? 0
      : Math.min(...strokes.map((stroke) => pathOnGlyph(raster, stroke)));

  let problem: string | undefined;
  if (strokes.length === 0) {
    problem = `"${character}" produced no strokes`;
  } else if (coverage < MIN_COVERAGE) {
    problem = `strokes cover only ${(coverage * 100).toFixed(1)}% of "${character}" (need ${MIN_COVERAGE * 100}%)`;
  } else if (worstOnGlyph < MIN_ON_GLYPH) {
    problem = `a stroke runs ${((1 - worstOnGlyph) * 100).toFixed(1)}% outside "${character}" (need ${MIN_ON_GLYPH * 100}% inside)`;
  }

  return { contours, strokes, coverage, worstOnGlyph, problem };
}

for (const [id, character, animation] of CHARACTERS) {
  const source = UPENN[animation ?? ''];
  if (!source) {
    throw new Error(`${id}: no writing animation for "${character}" (${animation})`);
  }
  const built = build(id, character, source);
  // An animated letter that stops fitting its own outline is a regression in
  // the pipeline, not a letter to quietly drop.
  if (built.problem) throw new Error(`${id}: ${built.problem}`);
  extracted.push({ id, character, contours: built.contours, strokes: built.strokes });
  console.log(
    `${id} ${character}: ${built.strokes.length} stroke(s) from ${animation}, ${(built.coverage * 100).toFixed(1)}% covered, ${(built.worstOnGlyph * 100).toFixed(1)}% on the letter`
  );
}

const animated = new Set(extracted.map((glyph) => glyph.id));
const refused: string[] = [];

for (const [id, source] of Object.entries(COMPOSED)) {
  // A letter UPenn animates is always preferred to the same letter composed.
  if (animated.has(id)) continue;
  const built = build(id, source.glyph, source);
  if (built.problem) {
    refused.push(`${id} ${source.glyph}: ${built.problem}`);
    continue;
  }
  extracted.push({ id, character: source.glyph, contours: built.contours, strokes: built.strokes });
  console.log(
    `${id} ${source.glyph}: ${built.strokes.length} stroke(s) composed, ${(built.coverage * 100).toFixed(1)}% covered, ${(built.worstOnGlyph * 100).toFixed(1)}% on the letter`
  );
}

if (refused.length > 0) {
  console.log(`\n${refused.length} composed letter(s) refused:`);
  for (const line of refused) console.log(`  ${line}`);
}

const points = (list: readonly Point[]): string =>
  `[${list.map((p) => `{x:${p.x},y:${p.y}}`).join(',')}]`;

const body = extracted
  .map(
    (glyph) => `  {
    characterId: ${JSON.stringify(glyph.id)},
    glyph: ${JSON.stringify(glyph.character)},
    contours: [
${glyph.contours.map((contour) => `      ${points(contour)},`).join('\n')}
    ],
    strokes: [
${glyph.strokes.map((stroke) => `      ${points(stroke)},`).join('\n')}
    ],
  },`
  )
  .join('\n');

writeFileSync(
  OUT,
  `// GENERATED by scripts/extract-tamil-glyphs.ts -- do not edit by hand.
//
// The letters the course guides, taken from the bundled
// NotoSansTamil-Regular.ttf and normalized into the tracing pad's box
// (fractions, y downward, ${MARGIN} margin, aspect preserved).
//
// \`contours\` is the outline the pad fills; \`strokes\` is the centre line found
// by thinning that same outline. The pad prints the outline rather than
// drawing the letter as text, so the printed letter, the stroke guide and the
// marking are one geometry -- which is what makes "does this path follow the
// letter" a measurement instead of an opinion.
import type { GlyphOutline } from './glyphGeometry';
import type { Point } from './strokes';

export type GeneratedGlyph = GlyphOutline & {
  characterId: string;
  glyph: string;
  /** Centre-line paths, in the script's default order. */
  strokes: readonly (readonly Point[])[];
};

export const GENERATED_GLYPHS: readonly GeneratedGlyph[] = [
${body}
];
`,
  'utf8'
);

console.log(`\nwrote ${OUT}`);
