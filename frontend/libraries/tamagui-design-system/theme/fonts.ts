// Tamagui fonts: the family stacks and the weight scale.
//
// Every variable group a font declares must be keyed the SAME way: Tamagui's
// registerFontVariables walks size, lineHeight and letterSpacing together, so
// a size key with no matching lineHeight makes createCSSVariable hash an
// undefined name and the whole config fails to load -- which showed up only as
// `Missing "themes" in your tamagui.config file` and an `expo export` that
// exited 0 having written no index.html.
import { defaultConfig } from '@tamagui/config/v4';

import { createFont } from '../primitives';

import { tokens } from './tokens';

const FAMILIES = {
  sans: 'system-ui, -apple-system, Roboto, sans-serif',
  serif: 'ui-serif, Georgia, serif',
  mono: 'Menlo, Courier, monospace',
  // the app loads it by this name (tamil-writing's useTamilFont); no stack, so a template never falls back to another shape
  tamil: 'NotoSansTamil',
} as const;

const NAMED_WEIGHTS = {
  thin: '100',
  extralight: '200',
  light: '300',
  normal: '400',
  medium: '500',
  semibold: '600',
  bold: '700',
  extrabold: '800',
  black: '900',
} as const;

const namedSizes = tokens.size;
type NamedSizeKey = keyof typeof namedSizes;

// 1.4x the font size is the ratio the old design system's type scale used.
const namedLineHeights = Object.fromEntries(
  (Object.keys(namedSizes) as NamedSizeKey[]).map((key) => [
    key,
    Math.round(namedSizes[key] * 1.4),
  ])
) as Record<NamedSizeKey, number>;

const namedLetterSpacing = Object.fromEntries(
  (Object.keys(namedSizes) as NamedSizeKey[]).map((key) => [key, 0])
) as Record<NamedSizeKey, number>;

function font(family: string) {
  const base = defaultConfig.fonts.body;
  return createFont({
    family,
    size: { ...base.size, ...namedSizes },
    lineHeight: { ...base.lineHeight, ...namedLineHeights },
    letterSpacing: { ...base.letterSpacing, ...namedLetterSpacing },
    weight: { ...base.weight, ...NAMED_WEIGHTS },
  });
}

export const fonts = {
  body: font(FAMILIES.sans),
  serif: font(FAMILIES.serif),
  mono: font(FAMILIES.mono),
  tamil: font(FAMILIES.tamil),
};
