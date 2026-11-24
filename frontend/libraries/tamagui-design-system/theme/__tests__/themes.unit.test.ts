import { describe, expect, it } from 'vitest';

import { PRESET_COLORS } from '../presetColors';
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

  it('when the interaction colors resolve, then quiet surfaces use muted tones in both appearances', () => {
    for (const theme of [themes.light, themes.dark]) {
      expect(theme.color4).toBe(theme.color3);
      expect(theme.color4).not.toBe(theme.color9);
    }
  });
});

describe('given the raw preset color records', () => {
  it('when themes are built, then preset colors resolve to rgb() strings or CSS keywords', () => {
    for (const [id, preset] of Object.entries(PRESET_COLORS)) {
      for (const [appearance, colors] of Object.entries(preset)) {
        const name = `${id}_${appearance}`;
        for (const key of Object.keys(colors)) {
          const value = themes[name][key];
          const isRgb = RGB_CSS_PATTERN.test(value);
          const isKeyword = ['transparent', 'currentColor', 'inherit'].includes(value);
          expect(isRgb || isKeyword, `${name}.${key} = ${value}`).toBe(true);
        }
      }
    }
  });
});
