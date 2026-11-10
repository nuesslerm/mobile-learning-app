import { describe, expect, it } from 'vitest';

import { themes } from '../themes';

const RGB_CSS_PATTERN = /^rgb\(\d+, \d+, \d+\)$/;

const PRESET_THEME_IDS = [
  'ios-native',
  'monochrome-pro',
  'classic-duolingo',
  'ocean-breeze',
  'forest-fresh',
  'desert-dawn',
  'pastel-pop',
  'midnight-neon',
  'royal-purple',
  'chess-wood',
  'sunset-vibes',
  'leetcode-slate',
] as const;

describe('given the twelve design-system preset themes', () => {
  it('when themes are built, then every preset id is present in both light and dark modes', () => {
    for (const presetId of PRESET_THEME_IDS) {
      expect(themes[`${presetId}_light`]).toBeDefined();
      expect(themes[`${presetId}_dark`]).toBeDefined();
    }
  });
});

describe('given the ios-native preset is the default', () => {
  it('when themes are built, then plain light and dark alias the ios-native themes', () => {
    expect(themes.light).toBe(themes['ios-native_light']);
    expect(themes.dark).toBe(themes['ios-native_dark']);
  });
});

describe('given the raw preset color records', () => {
  it('when themes are built, then every value is an rgb() string or a CSS keyword', () => {
    for (const [name, theme] of Object.entries(themes)) {
      for (const [key, value] of Object.entries(theme)) {
        const isRgb = RGB_CSS_PATTERN.test(value);
        const isKeyword = ['transparent', 'currentColor', 'inherit'].includes(
          value
        );
        expect(isRgb || isKeyword, `${name}.${key} = ${value}`).toBe(true);
      }
    }
  });
});
