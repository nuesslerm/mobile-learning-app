/**
 * Re-export only. The config and its `declare module '@tamagui/core'` augmentation
 * live in @lib/tamagui-design-system -- declaring a second one here merges
 * with it and loses resolved types (see that file's comment).
 */
export {
  tamaguiConfig,
  tamaguiConfig as default,
} from '@lib/tamagui-design-system/theme';
