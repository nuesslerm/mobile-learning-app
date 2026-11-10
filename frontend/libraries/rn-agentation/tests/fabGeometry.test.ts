// fabGeometry.test.ts
import { describe, expect, it } from 'vitest';

import {
  clamp01,
  fabBounds,
  positionToRatio,
  ratioToPosition,
} from '../src/geometry/fabGeometry';

const layout = { width: 400, height: 800, insets: { top: 50, bottom: 30, left: 0, right: 0 }, fabSize: 44, margin: 12 };

describe('fabGeometry', () => {
  it('clamp01 clamps into [0,1] and handles non-finite', () => {
    expect(clamp01(-0.5)).toBe(0);
    expect(clamp01(1.5)).toBe(1);
    expect(clamp01(0.5)).toBe(0.5);
    expect(clamp01(Number.NaN)).toBe(0);
  });

  it('fabBounds reserves insets + margin + fab size', () => {
    expect(fabBounds(layout)).toEqual({
      minX: 12, // insets.left + margin
      maxX: 400 - 44 - 0 - 12, // 344
      minY: 62, // insets.top + margin
      maxY: 800 - 44 - 30 - 12, // 714
    });
  });

  it('ratioToPosition maps 0..1 across the bounds', () => {
    const b = fabBounds(layout);
    expect(ratioToPosition(0, 0, b)).toEqual({ x: b.minX, y: b.minY });
    expect(ratioToPosition(1, 1, b)).toEqual({ x: b.maxX, y: b.maxY });
  });

  it('positionToRatio is the inverse and clamps out-of-range', () => {
    const b = fabBounds(layout);
    expect(positionToRatio(b.minX, b.minY, b)).toEqual({ xRatio: 0, yRatio: 0 });
    expect(positionToRatio(b.maxX, b.maxY, b)).toEqual({ xRatio: 1, yRatio: 1 });
    // beyond max → clamped to 1
    expect(positionToRatio(99999, 99999, b)).toEqual({ xRatio: 1, yRatio: 1 });
  });
});
