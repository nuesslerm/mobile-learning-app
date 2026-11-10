import { defaultConfig } from '@tamagui/config/v4';

import { createTamagui } from '../primitives';

import { fonts } from './fonts';
import { themes } from './themes';
import { tokens } from './tokens';

/**
 * The design system owns the Tamagui config, and the app imports it.
 *
 * This has to be a single declaration. Tamagui types shorthands, theme tokens
 * and animation names through a `declare module '@tamagui/core'` augmentation, and
 * TypeScript MERGES duplicate augmentations rather than letting the nearest
 * one win. When the app declared one from `createTamagui(defaultConfig)` and
 * this library declared another from the raw `defaultConfig`, the merge lost
 * the resolved animation names -- so `animation="quick"` type-errored from the
 * app while typechecking clean inside the library. One config, one
 * augmentation, no divergence.
 *
 * Starts from Tamagui's own defaults rather than a bespoke token set: the
 * point of this app is to see what Tamagui feels like unmodified. The design
 * system's transcribed tokens, themes and fonts override the defaults; token
 * groups merge with (rather than replace) the defaults because existing
 * components still reference the default numeric size/radius keys.
 */
export const tamaguiConfig = createTamagui({
  ...defaultConfig,
  tokens: {
    ...defaultConfig.tokens,
    space: { ...defaultConfig.tokens.space, ...tokens.space },
    size: { ...defaultConfig.tokens.size, ...tokens.size },
    radius: { ...defaultConfig.tokens.radius, ...tokens.radius },
    zIndex: { ...defaultConfig.tokens.zIndex, ...tokens.zIndex },
  },
  themes,
  fonts: { ...defaultConfig.fonts, ...fonts },
});

/**
 * Tamagui's build-time config loader looks for `default` or `config`, not an
 * arbitrary named export. Without this it evaluated the module to null, logged
 * `Missing "themes" in your tamagui.config file`, fell back to no custom
 * config, and let `expo export` exit 0 having written no index.html.
 */
export const config = tamaguiConfig;

export type AppConfig = typeof tamaguiConfig;

declare module '@tamagui/core' {
  // eslint-disable-next-line @typescript-eslint/no-empty-object-type
  interface TamaguiCustomConfig extends AppConfig {}
}
