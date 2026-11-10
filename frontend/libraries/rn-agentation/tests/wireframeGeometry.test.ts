import { describe, expect, it } from 'vitest';

import { ratioToPercent, rectToWireframe, verticalGap } from '../src/geometry/wireframeGeometry';

describe('rectToWireframe', () => {
  it('converts a rect to viewport-relative ratios', () => {
    const rect = { x: 100, y: 200, width: 300, height: 150 };
    const viewport = { width: 1000, height: 1000 };
    const w = rectToWireframe(rect, viewport, 'Card');
    expect(w.componentType).toBe('Card');
    expect(w.xRatio).toBe(0.1);
    expect(w.yRatio).toBe(0.2);
    expect(w.widthRatio).toBe(0.3);
    expect(w.heightRatio).toBe(0.15);
  });

  it('clamps ratios to [0, 1]', () => {
    const rect = { x: -100, y: 1200, width: 2000, height: 2000 };
    const viewport = { width: 1000, height: 1000 };
    const w = rectToWireframe(rect, viewport, 'Hero');
    expect(w.xRatio).toBe(0);
    expect(w.yRatio).toBe(1);
    expect(w.widthRatio).toBe(1);
    expect(w.heightRatio).toBe(1);
  });
});

describe('ratioToPercent', () => {
  it('formats a ratio as a percentage', () => {
    expect(ratioToPercent(0.1234)).toBe('12.3%');
  });
});

describe('verticalGap', () => {
  it('computes the pixel gap between two placements', () => {
    const prev = rectToWireframe(
      { x: 0, y: 0, width: 100, height: 100 },
      { width: 1000, height: 1000 },
      'Header',
    );
    const next = rectToWireframe(
      { x: 0, y: 250, width: 100, height: 100 },
      { width: 1000, height: 1000 },
      'Card',
    );
    expect(verticalGap(prev, next, 1000)).toBe(150);
  });
});
