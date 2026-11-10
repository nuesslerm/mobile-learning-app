import { describe, expect, it } from 'vitest';

import { guideFontSize } from '../guideSize';

describe('guideFontSize', () => {
  const pad = { width: 365, height: 392 };

  it('given a word 500 px wide at its base size on a 365 px pad, when sized, then it takes 90 % of the pad width', () => {
    // Given / When
    const size = guideFontSize(pad, 0.34, 500);

    // Then the base size, 0.34 of the shorter side, shrinks by 365 x 0.9 / 500
    expect(size).toBeCloseTo((365 * 0.34 * 365 * 0.9) / 500, 5);
  });

  it('given a letter 200 px wide, when sized, then it keeps its share of the shorter side', () => {
    expect(guideFontSize(pad, 0.82, 200)).toBeCloseTo(365 * 0.82, 5);
  });

  it('given a word not measured yet, when sized, then it starts at its base size', () => {
    expect(guideFontSize(pad, 0.34, 0)).toBeCloseTo(365 * 0.34, 5);
  });

  it('given a pad not laid out yet, when sized, then it falls back to the smallest size', () => {
    expect(guideFontSize({ width: 0, height: 0 }, 0.34, 500)).toBe(24);
  });
});
