import { Logger } from '@lib/logging';
import type { LucideIcon } from 'lucide-react-native';
import type { GestureResponderEvent } from 'react-native';

import {
  Button as TamaguiButton,
  type ButtonProps as TamaguiButtonProps,
} from '../primitives';

import { GlassControlBackground, supportsGlassControl } from './glass-control-background';
import { Icon, type IconProps } from './icon';

export type ButtonVariant =
  | 'default'
  | 'destructive'
  | 'secondary'
  | 'outline'
  | 'ghost'
  | 'link'
  | 'success';

export type ButtonProps = Omit<TamaguiButtonProps, 'variant'> & {
  variant?: ButtonVariant;
  /** Lucide icon rendered before the label. */
  icon?: LucideIcon;
  iconSize?: number;
  iconFilled?: boolean;
  iconSystemSymbol?: IconProps['systemSymbol'];
  /** Uses a native glass capsule where supported. */
  glass?: boolean;
  /** Suppress the automatic press log. Use on rows that would log themselves. */
  pressLog?: 'off';
  testID?: string;
};

/**
 * Press feedback is opacity, not travel.
 *
 * App #1's Button animates `translateY` into a darker underlay -- the Duolingo
 * 3D grammar. That is the thing this second app exists to compare against, so
 * reproducing it would defeat the point. Tamagui's own pressStyle is used
 * instead, which is closer to how a native control behaves.
 *
 * Disabled feedback is opacity too, and it is not optional: `disabled` alone is
 * invisible in Tamagui, so a button without `disabledStyle` renders exactly like
 * an enabled one.
 */
const VARIANT_PROPS: Record<ButtonVariant, Partial<TamaguiButtonProps>> = {
  default: { theme: 'blue', bg: '$color9', color: '$color1' },
  destructive: { theme: 'red', bg: '$color9', color: '$color1' },
  secondary: { bg: '$color4', color: '$color' },
  outline: {
    bg: 'transparent',
    borderWidth: 1,
    borderColor: '$borderColor',
    color: '$color',
  },
  ghost: { bg: 'transparent', borderWidth: 0, color: '$color' },
  link: { bg: 'transparent', borderWidth: 0, color: '$blue10' },
  success: { bg: '$success', color: 'white' },
};

export function Button({
  variant = 'default',
  icon,
  iconSize = 18,
  iconFilled,
  iconSystemSymbol,
  glass,
  pressLog,
  onPress,
  testID,
  children,
  accessibilityLabel,
  ...rest
}: ButtonProps) {
  const nativeGlass = glass && supportsGlassControl();
  const handlePress = (event: GestureResponderEvent) => {
    if (pressLog !== 'off') Logger.info('ds.button.press', { testID, variant });
    onPress?.(event);
  };

  return (
    <TamaguiButton
      testID={testID}
      onPress={handlePress}
      pressStyle={{ opacity: 0.7 }}
      // A disabled button MUST look disabled. Without this, `disabled` only
      // sets aria-disabled and pointer-events: none -- computed opacity stays
      // 1 and the control is pixel-identical to an enabled one beside it. On a
      // device that is a control the user taps repeatedly while nothing
      // happens and nothing explains why. 0.4 matches app #1's
      // `disabled:opacity-40`, so the two design systems agree.
      disabledStyle={{ opacity: 0.4 }}
      {...VARIANT_PROPS[variant]}
      {...rest}
      {...(nativeGlass ? { style: [rest.style, { backgroundColor: 'transparent', borderWidth: 0 }] } : {})}
      // Tamagui hands RN's accessibilityLabel to the DOM as an unknown attribute on web; aria-label works on both
      aria-label={rest['aria-label'] ?? accessibilityLabel}
    >
      {nativeGlass ? <GlassControlBackground /> : null}
      {icon ? <Icon as={icon} size={iconSize} filled={iconFilled} systemSymbol={iconSystemSymbol} /> : null}
      {children}
    </TamaguiButton>
  );
}
