import { describe, expect, it } from 'vitest';

import { getStrokeGuide, guidedCharacterIds, hasStrokeGuide } from '../strokeGuide';
import {
  END_TOLERANCE,
  PATH_TOLERANCE,
  START_TOLERANCE,
  assessStroke,
  meanDeviation,
  normalizeStroke,
  pathLength,
  resample,
} from '../strokeValidation';
import type { Point, Stroke } from '../strokes';

const CANVAS = { width: 300, height: 300 };

/** A reference stroke's path, drawn perfectly, in pad pixels. */
function traced(points: readonly Point[], canvas = CANVAS): Stroke {
  return points.map((point) => ({
    x: point.x * canvas.width,
    y: point.y * canvas.height,
  }));
}

/**
 * The first `share` of a stroke BY LENGTH. Slicing by point count does not cut
 * a stroke short: the points bunch up on the curves, so the first third of them
 * can be most of the line.
 */
function firstPart(points: readonly Point[], share: number): Point[] {
  const target = pathLength([...points]) * share;
  const out: Point[] = [points[0] as Point];
  let run = 0;
  for (let i = 1; i < points.length; i += 1) {
    const a = points[i - 1] as Point;
    const b = points[i] as Point;
    run += Math.hypot(b.x - a.x, b.y - a.y);
    out.push(b);
    if (run >= target) break;
  }
  return out.length >= 2 ? out : [...points].slice(0, 2);
}

function referenceOf(characterId: string, index = 0) {
  const stroke = getStrokeGuide(characterId)?.strokes[index];
  if (!stroke) throw new Error(`no reference stroke ${index} for ${characterId}`);
  return stroke;
}

describe('pathLength and resample', () => {
  it('Given a three-four-five path, When measured, Then the segments sum', () => {
    expect(
      pathLength([
        { x: 0, y: 0 },
        { x: 3, y: 4 },
        { x: 3, y: 9 },
      ])
    ).toBe(10);
  });

  it('Given a straight path, When resampled, Then the points are evenly spread and the ends are kept', () => {
    const points = resample([{ x: 0, y: 0 }, { x: 10, y: 0 }], 5);
    expect(points).toHaveLength(5);
    expect(points[0]).toEqual({ x: 0, y: 0 });
    expect(points[4]).toEqual({ x: 10, y: 0 });
    expect(points[2]?.x).toBeCloseTo(5);
  });

  it('Given a zero-length path, When resampled, Then it yields that point rather than dividing by zero', () => {
    const points = resample([{ x: 2, y: 2 }, { x: 2, y: 2 }], 4);
    expect(points).toHaveLength(4);
    expect(points.every((point) => point.x === 2 && point.y === 2)).toBe(true);
  });
});

describe('normalizeStroke', () => {
  it('Given pad pixels, When normalized, Then they become fractions of the pad', () => {
    expect(normalizeStroke([{ x: 150, y: 75 }], CANVAS)).toEqual([
      { x: 0.5, y: 0.25 },
    ]);
  });

  it('Given a pad that has not been laid out, When normalized, Then nothing comes back rather than Infinity', () => {
    expect(normalizeStroke([{ x: 1, y: 1 }], { width: 0, height: 0 })).toEqual([]);
  });
});

describe('meanDeviation', () => {
  it('Given a path against itself, When measured, Then the deviation is zero', () => {
    const path = referenceOf('uyir-a').points;
    expect(meanDeviation(path, path)).toBeCloseTo(0);
  });

  it('Given a short scribble sitting on the reference, When measured, Then it is NOT scored as a match', () => {
    const reference = [
      { x: 0.1, y: 0.5 },
      { x: 0.9, y: 0.5 },
    ];
    const scribble = [
      { x: 0.1, y: 0.5 },
      { x: 0.2, y: 0.5 },
    ];
    // Measured only one way this scores ~0: every scribble point is on the
    // line. Measured both ways, the reference's far end is nowhere near it.
    expect(meanDeviation(scribble, reference)).toBeGreaterThan(0.2);
  });
});

describe('assessStroke', () => {
  const reference = referenceOf('uyir-a');

  it('Given the reference stroke traced exactly, When assessed, Then it passes with a top score', () => {
    const result = assessStroke(traced(reference.points), reference, CANVAS);
    expect(result.ok).toBe(true);
    expect(result.deviation).toBeCloseTo(0, 2);
    expect(result.score).toBeGreaterThan(0.95);
    expect(result.message).toBe('Right on the line.');
  });

  it('Given the reference traced at a different pad size, When assessed, Then it still passes', () => {
    const big = { width: 900, height: 900 };
    const result = assessStroke(traced(reference.points, big), reference, big);
    expect(result.ok).toBe(true);
  });

  it('Given the stroke traced backwards, When assessed, Then it is refused for direction, not for its start', () => {
    const result = assessStroke(
      traced([...reference.points].reverse()),
      reference,
      CANVAS
    );
    expect(result.ok).toBe(false);
    expect(result.reason).toBe('backwards');
    expect(result.message).toContain('wrong way');
  });

  it('Given a stroke cut short, When assessed, Then it is refused for length', () => {
    const half = firstPart(reference.points, 0.3);
    const result = assessStroke(traced(half), reference, CANVAS);
    expect(result.ok).toBe(false);
    expect(result.reason).toBe('too-short');
  });

  it('Given a stroke stopped half way along the line, When assessed, Then it is refused even though every point it drew was right', () => {
    // The failure this test exists for: length alone accepted this at 89%,
    // because a half stroke that sits ON the reference has almost no
    // deviation. It stopped; that is the whole problem with it.
    //
    // A straight synthetic stroke rather than a letter's, so regenerating the
    // glyph data cannot quietly turn this back into a passing case.
    const straight = {
      points: [
        { x: 0.3, y: 0.2 },
        { x: 0.3, y: 0.5 },
        { x: 0.3, y: 0.8 },
      ],
      hint: 'straight down',
    };
    const half = [
      { x: 0.3, y: 0.2 },
      { x: 0.3, y: 0.5 },
    ];
    const result = assessStroke(traced(half), straight, CANVAS);
    expect(result.ok).toBe(false);
    expect(result.reason).toBe('too-short');
  });

  it('Given a single tap, When assessed, Then it is refused rather than crashing', () => {
    const result = assessStroke([{ x: 10, y: 10 }], reference, CANVAS);
    expect(result.ok).toBe(false);
    expect(result.reason).toBe('too-short');
    expect(result.score).toBe(0);
  });

  it('Given the right shape started well away from the dot, When assessed, Then it is refused for its start', () => {
    const shifted = reference.points.map((point) => ({
      x: point.x + START_TOLERANCE + 0.1,
      y: point.y,
    }));
    const result = assessStroke(traced(shifted), reference, CANVAS);
    expect(result.ok).toBe(false);
    expect(result.reason).toBe('wrong-start');
  });

  it('Given a stroke that starts right and then wanders, When assessed, Then it is refused for leaving the path', () => {
    const start = reference.points[0];
    const end = reference.points[reference.points.length - 1];
    if (!start || !end) throw new Error('reference stroke has no endpoints');
    // Right start, right finish, long enough -- and a detour through the far
    // corner in between. The only thing wrong with it is the path it took.
    // Synthetic, so regenerating the glyph data cannot turn this into a
    // length failure instead.
    const straight = {
      points: [
        { x: 0.2, y: 0.5 },
        { x: 0.5, y: 0.5 },
        { x: 0.8, y: 0.5 },
      ],
      hint: 'across',
    };
    const detour = [
      { x: 0.2, y: 0.5 },
      { x: 0.35, y: 0.08 },
      { x: 0.65, y: 0.08 },
      { x: 0.8, y: 0.5 },
    ];
    void start;
    void end;
    const result = assessStroke(traced(detour), straight, CANVAS);
    expect(result.ok).toBe(false);
    expect(result.reason).toBe('off-path');
  });

  it('Given a trace that stops a hair early, When assessed, Then it still passes -- a finger lifts where it lifts', () => {
    const straight = {
      points: [
        { x: 0.3, y: 0.2 },
        { x: 0.3, y: 0.8 },
      ],
      hint: 'straight down',
    };
    const result = assessStroke(
      traced([
        { x: 0.3, y: 0.2 },
        { x: 0.3, y: 0.76 },
      ]),
      straight,
      CANVAS
    );
    expect(result.ok).toBe(true);
  });

  it('Given a closed loop traced backwards, When assessed, Then it is still caught -- its endpoints are the same point', () => {
    const loop = {
      points: [
        { x: 0.5, y: 0.3 },
        { x: 0.65, y: 0.45 },
        { x: 0.5, y: 0.6 },
        { x: 0.35, y: 0.45 },
        { x: 0.5, y: 0.3 },
      ],
      hint: 'round the loop',
    };
    const result = assessStroke(
      traced([...loop.points].reverse()),
      loop,
      CANVAS
    );
    expect(result.ok).toBe(false);
    expect(result.reason).toBe('backwards');
  });

  it('Given a trace that abandons the stroke part way, When assessed, Then it is refused', () => {
    const result = assessStroke(
      traced(firstPart(reference.points, 0.4)),
      reference,
      CANVAS
    );
    expect(result.ok).toBe(false);
    expect(result.reason).toBe('too-short');
    expect(END_TOLERANCE).toBeGreaterThan(0);
  });

  it('Given a slightly unsteady trace, When assessed, Then it still passes -- a hand is not a plotter', () => {
    const shaky = reference.points.map((point, index) => ({
      x: point.x + (index % 2 === 0 ? 0.015 : -0.015),
      y: point.y + (index % 2 === 0 ? -0.015 : 0.015),
    }));
    const result = assessStroke(traced(shaky), reference, CANVAS);
    expect(result.ok).toBe(true);
    expect(result.deviation).toBeLessThan(PATH_TOLERANCE);
  });
});

describe('the authored guides', () => {
  it('Given the course, When guides are counted, Then every letter with a writing animation carries one', () => {
    // 12 uyir + ஃ + the 18 bare consonants + 97 combinations.
    expect(guidedCharacterIds()).toHaveLength(110);
    expect(hasStrokeGuide('uyir-a')).toBe(true);
    expect(hasStrokeGuide('uyirmei-k-a')).toBe(true);
    // கோ has an animation of its own; மோ is composed from ம's animation and
    // the sign as the க series writes it.
    expect(hasStrokeGuide('uyirmei-k-oo')).toBe(true);
    expect(hasStrokeGuide('uyirmei-m-oo')).toBe(true);
    // ை is not animated anywhere, and கு-shaped ligatures leave no consonant
    // glyph to compose onto.
    expect(hasStrokeGuide('uyirmei-m-ai')).toBe(false);
    expect(hasStrokeGuide('uyirmei-m-u')).toBe(false);
  });

  it('Given every guide, When its strokes are read, Then each has a path and a hint', () => {
    for (const id of guidedCharacterIds()) {
      const guide = getStrokeGuide(id);
      expect(guide?.strokes.length).toBeGreaterThan(0);
      for (const stroke of guide?.strokes ?? []) {
        expect(stroke.points.length).toBeGreaterThanOrEqual(2);
        expect(stroke.hint.length).toBeGreaterThan(0);
      }
    }
  });

  it('Given every guide stroke, When its points are read, Then all of them sit inside the pad', () => {
    for (const id of guidedCharacterIds()) {
      for (const stroke of getStrokeGuide(id)?.strokes ?? []) {
        for (const point of stroke.points) {
          expect(point.x).toBeGreaterThanOrEqual(0);
          expect(point.x).toBeLessThanOrEqual(1);
          expect(point.y).toBeGreaterThanOrEqual(0);
          expect(point.y).toBeLessThanOrEqual(1);
        }
      }
    }
  });

  it('Given every guide stroke, When traced exactly, Then its own validator accepts it', () => {
    for (const id of guidedCharacterIds()) {
      for (const stroke of getStrokeGuide(id)?.strokes ?? []) {
        const result = assessStroke(traced(stroke.points), stroke, CANVAS);
        expect(`${id}: ${result.reason ?? 'ok'}`).toBe(`${id}: ok`);
      }
    }
  });

  it('Given every guide stroke, When traced backwards, Then its own validator refuses it', () => {
    for (const id of guidedCharacterIds()) {
      for (const stroke of getStrokeGuide(id)?.strokes ?? []) {
        const result = assessStroke(
          traced([...stroke.points].reverse()),
          stroke,
          CANVAS
        );
        expect(`${id}: ${result.ok}`).toBe(`${id}: false`);
      }
    }
  });
});
