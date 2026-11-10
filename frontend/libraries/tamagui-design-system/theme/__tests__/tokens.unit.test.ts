import { describe, expect, it } from 'vitest';

import { tokens } from '../tokens';

const SPACING = {
  0: '0px',
  0.5: '2px',
  1: '4px',
  1.5: '6px',
  2: '8px',
  2.5: '10px',
  3: '12px',
  3.5: '14px',
  4: '16px',
  4.5: '18px',
  5: '20px',
  5.5: '22px',
  6: '24px',
  6.5: '26px',
  7: '28px',
  7.5: '30px',
  8: '32px',
  8.5: '34px',
  9: '36px',
  9.5: '38px',
  10: '40px',
  10.5: '42px',
  11: '44px',
  11.5: '46px',
  12: '48px',
  12.5: '50px',
  13: '52px',
  13.5: '54px',
  14: '56px',
  14.5: '58px',
  15: '60px',
  15.5: '62px',
  16: '64px',
  16.5: '66px',
  17: '68px',
  17.5: '70px',
  18: '72px',
  18.5: '74px',
  19: '76px',
  19.5: '78px',
  20: '80px',
  24: '96px',
  28: '112px',
  32: '128px',
  36: '144px',
  40: '160px',
  44: '176px',
  48: '192px',
  52: '208px',
  56: '224px',
  60: '240px',
  64: '256px',
  72: '288px',
  80: '320px',
  96: '384px',
  'bottom-safe': '120px',
  px: '1px',
} as const;

const BASE_FONT_SIZE_PX = {
  '5xs': 7,
  '4xs': 8,
  '3xs': 9,
  '2xs': 10,
  xs: 12,
  sm: 13,
  base: 14,
  lg: 16,
  xl: 18,
  '2xl': 21,
  '3xl': 24,
  '4xl': 36,
  '5xl': 48,
  '6xl': 60,
  '7xl': 72,
} as const;

const RADIUS = {
  none: '0',
  sm: '0.25rem',
  md: '0.5rem',
  lg: '0.75rem',
  pill: '1rem',
  xl: '1.5rem',
  nav: '0.75rem',
  tile: '1rem',
  'card-lg': '1.25rem',
} as const;

const Z_LAYERS = {
  base: 0,
  header: 5,
  docked: 10,
  sticky: 15,
  nav: 20,
  sheet: 25,
  overlay: 30,
  'debug-surface': 40,
} as const;

describe('given the design-system spacing source', () => {
  it('when tokens are transcribed, then every spacing key survives as its px number', () => {
    for (const [key, value] of Object.entries(SPACING) as [
      keyof typeof tokens.space,
      string,
    ][]) {
      expect(tokens.space[key]).toBe(parseFloat(value));
    }
  });
});

describe('given the design-system base font-size source', () => {
  it('when tokens are transcribed, then every font-size key survives with its px number', () => {
    for (const [key, value] of Object.entries(BASE_FONT_SIZE_PX) as [
      keyof typeof tokens.size,
      number,
    ][]) {
      expect(tokens.size[key]).toBe(value);
    }
  });
});

describe('given the design-system radius source', () => {
  it('when tokens are transcribed, then every radius key survives as rem converted at 16px per rem', () => {
    for (const [key, value] of Object.entries(RADIUS) as [
      keyof typeof tokens.radius,
      string,
    ][]) {
      expect(tokens.radius[key]).toBe(parseFloat(value) * 16);
    }
  });
});

describe('given the design-system z-layer source', () => {
  it('when tokens are transcribed, then every layer key survives with its number', () => {
    for (const [key, value] of Object.entries(Z_LAYERS) as [
      keyof typeof tokens.zIndex,
      number,
    ][]) {
      expect(tokens.zIndex[key]).toBe(value);
    }
  });
});
