// Tamagui themes built from the twelve presets this package vendors.
// RawThemeColors values are `R G B` strings; every such value becomes a CSS
// `rgb(r, g, b)` string, keywords pass through.
//
// Each theme EXTENDS Tamagui's own theme for that mode rather than replacing
// it. The preset palettes carry shadcn names (background, card, muted,
// destructive, ...) while Tamagui's components -- and this package's own, e.g.
// badge.tsx's `borderColor: '$borderColor'` -- resolve Tamagui's names
// (`$background`, `$borderColor`, `$color1`..`$color12`). Replacing the
// defaults left those lookups undefined, so the export rendered with
// transparent backgrounds: `expo export` succeeded, every unit and flow test
// passed, and the design system came out unthemed. The semantic names are
// layered on top, and the ones with a Tamagui equivalent are mapped across.
import { defaultConfig } from '@tamagui/config/v4';

import { PRESET_COLORS, type RawThemeColors } from './presetColors';
import { PRESET_THEMES } from './presetThemes';

type TamaguiTheme = Record<string, string>;

const RGB_PATTERN = /^\d+ \d+ \d+$/;

function css(value: string): string {
  return RGB_PATTERN.test(value) ? `rgb(${value.split(' ').join(', ')})` : value;
}

/** Preset name -> the Tamagui name components actually resolve. */
const TAMAGUI_EQUIVALENT: Record<string, string[]> = {
  background: ['background', 'color1', 'background0'],
  foreground: ['color', 'color12'],
  card: ['color2', 'background025'],
  'card-foreground': ['color11'],
  border: ['borderColor', 'color5'],
  input: ['borderColorFocus', 'color6'],
  ring: ['outlineColor'],
  primary: ['color9'],
  'primary-foreground': ['color1'],
  muted: ['color3'],
  'muted-foreground': ['color10'],
  accent: ['color4'],
  popover: ['color2'],
};

function toTamaguiTheme(colors: RawThemeColors, mode: 'light' | 'dark'): TamaguiTheme {
  const theme: TamaguiTheme = { ...(defaultConfig.themes[mode] as unknown as TamaguiTheme) };
  for (const [key, value] of Object.entries(colors)) {
    const resolved = css(value);
    theme[key] = resolved;
    for (const alias of TAMAGUI_EQUIVALENT[key] ?? []) theme[alias] = resolved;
  }
  return theme;
}

export const themes: Record<string, TamaguiTheme> = (() => {
  const result: Record<string, TamaguiTheme> = {};
  for (const preset of PRESET_THEMES) {
    const colors = PRESET_COLORS[preset.id];
    result[`${preset.id}_light`] = toTamaguiTheme(colors.light, 'light');
    result[`${preset.id}_dark`] = toTamaguiTheme(colors.dark, 'dark');
  }
  const defaultPreset = PRESET_THEMES[0];
  result.light = result[`${defaultPreset.id}_light`];
  result.dark = result[`${defaultPreset.id}_dark`];
  return result;
})();
