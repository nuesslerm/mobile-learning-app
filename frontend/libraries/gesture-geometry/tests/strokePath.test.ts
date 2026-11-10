import { describe, expect, it } from 'vitest';

import {
  curveSamples,
  polylineSamples,
  strokeSamples,
  toPageCoordinates,
  transformPath,
} from '../src/strokePath';

const BOX = { x: 100, y: 200, width: 300, height: 400 };

type Point = { x: number; y: number };

function length(points: readonly Point[]): number {
  let total = 0;
  for (let i = 1; i < points.length; i += 1) {
    total += Math.hypot(
      at(points, i).x - at(points, i - 1).x,
      at(points, i).y - at(points, i - 1).y
    );
  }
  return total;
}

/** Indexing a sample list under `noUncheckedIndexedAccess`, loudly. */
function at(points: readonly Point[], index: number): Point {
  const point = points[index];
  if (!point) throw new Error(`no sample at index ${index} of ${points.length}`);
  return point;
}

describe('toPageCoordinates', () => {
  it('Given fractions of the element box, When mapped, Then they land inside that box on the page', () => {
    expect(
      toPageCoordinates([{ x: 0, y: 0 }, { x: 0.5, y: 0.5 }, { x: 1, y: 1 }], BOX)
    ).toEqual([
      { x: 100, y: 200 },
      { x: 250, y: 400 },
      { x: 400, y: 600 },
    ]);
  });

  it('Given the same fractions and a box twice the size, When mapped, Then the path keeps its shape', () => {
    const path = [{ x: 0.25, y: 0.75 }, { x: 0.75, y: 0.25 }];
    const small = toPageCoordinates(path, { x: 0, y: 0, width: 100, height: 100 });
    const large = toPageCoordinates(path, { x: 0, y: 0, width: 200, height: 200 });
    const slope = (p: Point[]) =>
      (at(p, 1).y - at(p, 0).y) / (at(p, 1).x - at(p, 0).x);
    expect(slope(small)).toBeCloseTo(slope(large));
  });

  it('Given pixel units, When mapped, Then points are offsets from the box top-left', () => {
    expect(toPageCoordinates([{ x: 10, y: 20 }], BOX, 'px')).toEqual([
      { x: 110, y: 220 },
    ]);
  });
});

describe('polylineSamples', () => {
  it('Given two points, When sampled, Then the first point plus one sample per step comes back', () => {
    const samples = polylineSamples([{ x: 0, y: 0 }, { x: 10, y: 0 }], 5);
    expect(samples).toHaveLength(6);
    expect(at(samples, 0)).toEqual({ x: 0, y: 0 });
    expect(at(samples, 5)).toEqual({ x: 10, y: 0 });
  });

  it('Given a three-point path, When sampled, Then every given point is hit exactly', () => {
    const samples = polylineSamples(
      [{ x: 0, y: 0 }, { x: 10, y: 10 }, { x: 20, y: 0 }],
      4
    );
    expect(samples).toHaveLength(9);
    expect(at(samples, 4)).toEqual({ x: 10, y: 10 });
    expect(at(samples, 8)).toEqual({ x: 20, y: 0 });
  });

  it('Given a straight path, When sampled, Then no sample leaves the line', () => {
    const samples = polylineSamples([{ x: 0, y: 0 }, { x: 30, y: 30 }], 6);
    for (const sample of samples) expect(sample.y).toBeCloseTo(sample.x);
  });
});

describe('curveSamples', () => {
  const ARC = [
    { x: 0, y: 0 },
    { x: 10, y: 10 },
    { x: 20, y: 0 },
  ];

  it('Given points to curve through, When sampled, Then each given point still lies on the path', () => {
    const samples = curveSamples(ARC, 8);
    expect(at(samples, 0)).toEqual({ x: 0, y: 0 });
    expect(at(samples, 8).x).toBeCloseTo(10);
    expect(at(samples, 8).y).toBeCloseTo(10);
    expect(at(samples, 16).x).toBeCloseTo(20);
    expect(at(samples, 16).y).toBeCloseTo(0);
  });

  it('Given an arc, When sampled, Then the curve bows away from the straight chord', () => {
    const curved = curveSamples(ARC, 8);
    const straight = polylineSamples(ARC, 8);
    // Quarter of the way along, a spline through these points rides above the
    // two chords -- if it did not, `curve` would be doing nothing.
    expect(at(curved, 4).y).toBeGreaterThan(at(straight, 4).y);
  });

  it('Given only two points, When curved, Then it is the straight line, not an invented bow', () => {
    const points = [{ x: 0, y: 0 }, { x: 10, y: 4 }];
    expect(curveSamples(points, 5)).toEqual(polylineSamples(points, 5));
  });

  it('Given a closed-ish path, When sampled, Then the direction actually reverses', () => {
    const samples = curveSamples(
      [{ x: 0, y: 0 }, { x: 10, y: 10 }, { x: 0, y: 20 }],
      6
    );
    const first = at(samples, 1).x - at(samples, 0).x;
    const last =
      at(samples, samples.length - 1).x - at(samples, samples.length - 2).x;
    expect(first).toBeGreaterThan(0);
    expect(last).toBeLessThan(0);
  });
});

describe('strokeSamples', () => {
  it('Given curve off, When sampled, Then it matches the polyline', () => {
    const points = [{ x: 0, y: 0 }, { x: 4, y: 4 }, { x: 8, y: 0 }];
    expect(strokeSamples(points, { samplesPerSegment: 3 })).toEqual(
      polylineSamples(points, 3)
    );
  });

  it('Given fewer than two points, When sampled, Then it refuses rather than drawing nothing', () => {
    expect(() => strokeSamples([{ x: 1, y: 1 }])).toThrow(/at least 2 points/);
  });

  it('Given a non-positive sample count, When sampled, Then it refuses', () => {
    expect(() =>
      strokeSamples([{ x: 0, y: 0 }, { x: 1, y: 1 }], { samplesPerSegment: 0 })
    ).toThrow(/positive integer/);
  });

  it('Given a non-finite point, When sampled, Then it refuses rather than moving the pointer to NaN', () => {
    expect(() =>
      strokeSamples([{ x: 0, y: 0 }, { x: Number.NaN, y: 1 }])
    ).toThrow(/not finite/);
  });
});

describe('transformPath', () => {
  const PATH = [
    { x: 0, y: 0 },
    { x: 1, y: 1 },
    { x: 2, y: 0 },
    { x: 3, y: 1 },
  ];

  it('Given no transform, When applied, Then the path comes back unchanged', () => {
    expect(transformPath(PATH)).toEqual(PATH);
  });

  it('Given reverse, When applied, Then the path is walked end-to-start', () => {
    const reversed = transformPath(PATH, { reverse: true });
    expect(at(reversed, 0)).toEqual({ x: 3, y: 1 });
    expect(at(reversed, reversed.length - 1)).toEqual({ x: 0, y: 0 });
  });

  it('Given reverse, When applied, Then the original array is not mutated', () => {
    const original = [...PATH];
    transformPath(PATH, { reverse: true });
    expect(PATH).toEqual(original);
  });

  it('Given an offset, When applied, Then every point shifts by it', () => {
    expect(transformPath(PATH, { offset: { x: 10, y: -5 } })).toEqual([
      { x: 10, y: -5 },
      { x: 11, y: -4 },
      { x: 12, y: -5 },
      { x: 13, y: -4 },
    ]);
  });

  it('Given a scale, When applied, Then the shape shrinks about its own centre rather than drifting', () => {
    const scaled = transformPath(PATH, { scale: 0.5 });
    const centre = (p: readonly Point[]) => ({
      x: (Math.min(...p.map((q) => q.x)) + Math.max(...p.map((q) => q.x))) / 2,
      y: (Math.min(...p.map((q) => q.y)) + Math.max(...p.map((q) => q.y))) / 2,
    });
    expect(centre(scaled).x).toBeCloseTo(centre(PATH).x);
    expect(centre(scaled).y).toBeCloseTo(centre(PATH).y);
    const width = (p: readonly Point[]) =>
      Math.max(...p.map((q) => q.x)) - Math.min(...p.map((q) => q.x));
    expect(width(scaled)).toBeCloseTo(width(PATH) / 2);
  });

  it('Given truncate, When applied, Then the path keeps its start and is half as long', () => {
    const cut = transformPath(PATH, { truncate: 0.5 });
    expect(at(cut, 0)).toEqual({ x: 0, y: 0 });
    expect(length(cut)).toBeCloseTo(length(PATH) / 2);
  });

  it('Given a two-point stroke, When truncated, Then it is genuinely cut -- not handed back whole', () => {
    const straight = [
      { x: 0, y: 0 },
      { x: 1, y: 0 },
    ];
    const cut = transformPath(straight, { truncate: 0.25 });
    expect(length(cut)).toBeCloseTo(0.25);
    expect(at(cut, cut.length - 1).x).toBeCloseTo(0.25);
  });

  it('Given a truncate small enough to erase the path, When applied, Then two points still survive', () => {
    expect(transformPath(PATH, { truncate: 0.001 }).length).toBeGreaterThanOrEqual(2);
  });

  it('Given truncate and reverse together, When applied, Then the cut happens first, from the original start', () => {
    const both = transformPath(PATH, { truncate: 0.5, reverse: true });
    expect(at(both, both.length - 1)).toEqual({ x: 0, y: 0 });
  });

  it('Given a truncated path, When measured, Then no point past the cut survives', () => {
    const cut = transformPath(PATH, { truncate: 0.3 });
    expect(cut.length).toBeLessThan(PATH.length);
  });

  it('Given a wobble, When applied twice with the same seed, Then the same shaky path comes back', () => {
    const a = transformPath(PATH, { wobble: 0.2, wobbleSeed: 7 });
    const b = transformPath(PATH, { wobble: 0.2, wobbleSeed: 7 });
    expect(a).toEqual(b);
    expect(a).not.toEqual(PATH);
  });

  it('Given a wobble, When applied, Then the endpoints stay put so a rejection is about the shape', () => {
    const wobbled = transformPath(PATH, { wobble: 0.3, wobbleSeed: 2 });
    expect(at(wobbled, 0)).toEqual(at(PATH, 0));
    expect(at(wobbled, wobbled.length - 1)).toEqual(at(PATH, PATH.length - 1));
  });

  it('Given an out-of-range truncate, When applied, Then it refuses rather than emptying the path', () => {
    expect(() => transformPath(PATH, { truncate: 0 })).toThrow(/truncate/);
    expect(() => transformPath(PATH, { truncate: 1.5 })).toThrow(/truncate/);
  });

  it('Given a zero scale, When applied, Then it refuses rather than collapsing the path to a dot', () => {
    expect(() => transformPath(PATH, { scale: 0 })).toThrow(/non-zero/);
  });
});
