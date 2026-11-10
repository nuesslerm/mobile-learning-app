import {
  ActivityIndicator,
  type StyleProp,
  type ViewStyle,
} from 'react-native';

import { useTheme } from '../primitives';

export type SpinnerProps = {
  size?: 'small' | 'large';
  /** A theme token name, e.g. `color11`. Resolved here because ActivityIndicator takes a plain string. */
  colorToken?: string;
  style?: StyleProp<ViewStyle>;
  testID?: string;
};

/**
 * Loading spinner over React Native's own ActivityIndicator.
 *
 * Deliberately NOT Tamagui's `Spinner`, which is `styled(ActivityIndicator)`.
 * That crashed on a real device the moment the problem-description sheet
 * rendered a Mermaid diagram:
 *
 *   View config getter callback for component `RCTActivityIndicatorView`
 *   must be a function (received `undefined`)
 *
 * Tamagui's native createComponent resolves a view config for the host
 * component it wraps, and that lookup comes back undefined for
 * ActivityIndicator under the New Architecture. Web never showed it -- there
 * is no view config there at all -- so this is the same web-passes/native-dies
 * shape as the keyboard toolbar.
 *
 * The rule this encodes: do not put Tamagui's `styled()` over a React Native
 * host component that is not a plain View. `no-restricted-imports` enforces it.
 */
export function Spinner({
  size = 'small',
  colorToken = 'color11',
  style,
  testID,
}: SpinnerProps) {
  const theme = useTheme();

  const color = (theme[colorToken] as { val?: string } | undefined)?.val;
  return (
    <ActivityIndicator
      size={size}
      color={color}
      style={style}
      testID={testID}
    />
  );
}
