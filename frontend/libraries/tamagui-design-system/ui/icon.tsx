import type { LucideIcon } from 'lucide-react-native';

import { useTheme } from '../primitives';

export type IconProps = {
  as: LucideIcon;
  size?: number;
  /** Tamagui theme token (e.g. `$color11`). Falls back to the active `$color`. */
  color?: string;
  /** Required by the repo's E2E-selector lint rule on every rendered icon. */
  testID?: string;
};

/**
 * Lucide wrapper. App #1 registers the icon with NativeWind's `cssInterop` so
 * `className` can colour it; Tamagui has no equivalent, so the colour is
 * resolved from the theme and passed as a prop instead.
 */
export function Icon({ as: Component, size = 20, color, testID }: IconProps) {
  const theme = useTheme();
  const resolved = color ? theme[color]?.get() : theme.color?.get();
  return <Component size={size} color={resolved as string} testID={testID} />;
}
