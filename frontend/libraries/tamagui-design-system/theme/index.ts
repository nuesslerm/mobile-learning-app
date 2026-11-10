export { tamaguiConfig } from './tamagui.config';
export {
  PRESET_THEMES,
  PRESET_THEME_CATEGORIES,
  getDefaultTheme,
  getPresetThemeById,
  type PresetTheme,
  type PresetThemeCategory,
} from './presetThemes';
export {
  FONT_PRESETS,
  getDefaultFontPreset,
  getFontPresetById,
  type FontPresetConfig,
  type FontWeightToken,
} from './fontPresets';
export * from './state';

import type { WithSpringConfig } from 'react-native-reanimated';

/**
 * Motion presets. Same three roles app #1 names, but expressed as SwiftUI's
 * duration/dampingRatio rather than raw stiffness/damping -- Reanimated 4
 * accepts both, and the former says what the motion is FOR.
 *
 * Note app #1's `springBouncy` has a damping ratio of 0.92, which does not
 * bounce. These values are named for what they actually do.
 */
export const springSnappy: WithSpringConfig = {
  duration: 300,
  dampingRatio: 0.9,
};
export const springSmooth: WithSpringConfig = {
  duration: 350,
  dampingRatio: 1,
};
export const springBouncy: WithSpringConfig = {
  duration: 450,
  dampingRatio: 0.72,
};

/** Stagger between list items on entrance (ms). */
export const STAGGER_DELAY_DEFAULT = 40;
