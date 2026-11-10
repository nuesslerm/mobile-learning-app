import { describe, expect, it } from 'vitest';

import {
  appendPoint,
  boundingBox,
  describeFit,
  inkLength,
  pointsToPath,
  pointsToSmoothPath,
  type Stroke,
} from '../strokes';

const CANVAS = { width: 300, height: 300 };

/** A square of ink centred on `center`, `size` across. */
function box(center: { x: number; y: number }, size: number): Stroke[] {
  const half = size / 2;
  return [
    [
      { x: center.x - half, y: center.y - half },
      { x: center.x + half, y: center.y - half },
      { x: center.x + half, y: center.y + half },
      { x: center.x - half, y: center.y + half },
      { x: center.x - half, y: center.y - half },
    ],
  ];
}

describe('pointsToPath', () => {
  it('Given no samples, When a path is built, Then it is empty', () => {
    expect(pointsToPath([])).toBe('');
  });

  it('Given a single tap, When a path is built, Then it is a zero-length line so the dot still renders', () => {
    expect(pointsToPath([{ x: 4, y: 6 }])).toBe('M 4 6 L 4 6');
  });

  it('Given several samples, When a path is built, Then it moves to the first and lines to the rest', () => {
    const path = pointsToPath([
      { x: 0, y: 0 },
      { x: 10, y: 5 },
      { x: 20, y: 5 },
    ]);
    expect(path).toBe('M 0 0 L 10 5 L 20 5');
  });

  it('Given fractional samples, When a path is built, Then coordinates are rounded to two decimals', () => {
    expect(pointsToPath([{ x: 1.23456, y: 7.6543 }])).toBe('M 1.23 7.65 L 1.23 7.65');
  });
});

describe('appendPoint', () => {
  it('Given a sample further than the jitter threshold, When appended, Then the stroke grows', () => {
    const stroke = appendPoint([{ x: 0, y: 0 }], { x: 0, y: 10 });
    expect(stroke).toHaveLength(2);
  });

  it('Given a sample inside the jitter threshold, When appended, Then the same stroke reference comes back', () => {
    const original: Stroke = [{ x: 0, y: 0 }];
    expect(appendPoint(original, { x: 0.2, y: 0.2 })).toBe(original);
  });

  it('Given an empty stroke, When a sample is appended, Then it starts the stroke', () => {
    expect(appendPoint([], { x: 3, y: 3 })).toEqual([{ x: 3, y: 3 }]);
  });
});

describe('boundingBox and inkLength', () => {
  it('Given no strokes, When the bounding box is read, Then it is null rather than an infinite box', () => {
    expect(boundingBox([])).toBeNull();
    expect(boundingBox([[]])).toBeNull();
  });

  it('Given strokes spanning a region, When the bounding box is read, Then it covers every point', () => {
    const bounds = boundingBox([
      [
        { x: 10, y: 20 },
        { x: 30, y: 20 },
      ],
      [{ x: 5, y: 50 }],
    ]);
    expect(bounds).toEqual({
      minX: 5,
      minY: 20,
      maxX: 30,
      maxY: 50,
      width: 25,
      height: 30,
    });
  });

  it('Given a three-four-five stroke, When ink is measured, Then it is the summed segment length', () => {
    expect(
      inkLength([
        [
          { x: 0, y: 0 },
          { x: 3, y: 4 },
          { x: 3, y: 9 },
        ],
      ])
    ).toBe(10);
  });
});

describe('describeFit', () => {
  it('Given nothing drawn, When the fit is described, Then it reports empty', () => {
    expect(describeFit([], CANVAS)).toBe('empty');
  });

  it('Given a canvas that has not been laid out yet, When the fit is described, Then it reports empty rather than dividing by zero', () => {
    expect(describeFit(box({ x: 5, y: 5 }, 4), { width: 0, height: 0 })).toBe('empty');
  });

  it('Given writing that fills the middle of the pad, When the fit is described, Then it is good', () => {
    expect(describeFit(box({ x: 150, y: 150 }, 150), CANVAS)).toBe('good');
  });

  it('Given tiny writing, When the fit is described, Then it asks for bigger', () => {
    expect(describeFit(box({ x: 150, y: 150 }, 30), CANVAS)).toBe('too-small');
  });

  it('Given writing spilling to the edges, When the fit is described, Then it asks for smaller', () => {
    expect(describeFit(box({ x: 150, y: 150 }, 299), CANVAS)).toBe('too-large');
  });

  it('Given well-sized writing pushed to one side, When the fit is described, Then it names the direction to move', () => {
    expect(describeFit(box({ x: 60, y: 150 }, 100), CANVAS)).toBe('off-left');
    expect(describeFit(box({ x: 240, y: 150 }, 100), CANVAS)).toBe('off-right');
    expect(describeFit(box({ x: 150, y: 60 }, 100), CANVAS)).toBe('off-top');
    expect(describeFit(box({ x: 150, y: 240 }, 100), CANVAS)).toBe('off-bottom');
  });
});

describe('pointsToSmoothPath', () => {
  it('Given fewer than three samples, When drawn, Then it falls back to straight segments', () => {
    expect(pointsToSmoothPath([{ x: 1, y: 2 }])).toBe('M 1 2 L 1 2');
    expect(
      pointsToSmoothPath([
        { x: 0, y: 0 },
        { x: 4, y: 4 },
      ])
    ).toBe('M 0 0 L 4 4');
  });

  it('Given a run of samples, When drawn, Then it is quadratics through their midpoints, not a polygon', () => {
    const path = pointsToSmoothPath([
      { x: 0, y: 0 },
      { x: 10, y: 0 },
      { x: 10, y: 10 },
      { x: 0, y: 10 },
    ]);
    expect(path.startsWith('M 0 0')).toBe(true);
    expect(path).not.toContain(' L ');
    expect(path).toContain('Q 10 0 10 5');
  });

  it('Given a path, When drawn, Then it still ends where the stroke ended', () => {
    const path = pointsToSmoothPath([
      { x: 0, y: 0 },
      { x: 5, y: 5 },
      { x: 9, y: 1 },
    ]);
    expect(path.endsWith('9 1')).toBe(true);
  });
});
