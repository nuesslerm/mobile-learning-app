import type { NativeStackNavigationOptions } from 'expo-router';

import { ClassicScreenHeader } from './classic-screen-header';
import { nativeHeaderActive } from './native-header/header-env';
import type {
  HeaderActionSpec,
  ScreenHeaderSpec,
} from './native-header/header-spec';
import {
  HeaderScreen,
  useNativeHeaderOptions,
} from './native-header/native-screen-header';
import { useHeaderEnv } from './native-header/use-header-env';

export type ScreenHeaderAction = HeaderActionSpec;
export type ScreenHeaderProps = ScreenHeaderSpec & {
  /**
   * The screen opts in to the native glass bar: on iOS 26 with the Glass top bars setting on, the bar is react-native-screens header items and
   * this component renders nothing in the screen. Everywhere else the Tamagui header draws as it always did.
   */
  glass?: boolean;
  /** 'body' (the default): the Tamagui header draws in the screen. 'navigator': the bar is declared to the navigator through Stack.Screen options and nothing draws in the body. */
  placement?: 'body' | 'navigator';
};

function classicOptions(spec: ScreenHeaderSpec): NativeStackNavigationOptions {
  return { header: () => <ClassicScreenHeader {...spec} /> };
}

/** The one call site of a top bar. */
export function ScreenHeader({
  glass,
  placement = 'body',
  ...spec
}: ScreenHeaderProps) {
  const env = useHeaderEnv(glass === true);
  const native = useNativeHeaderOptions(spec, nativeHeaderActive(env));
  if (native) return <HeaderScreen options={native} />;
  if (placement === 'navigator')
    return <HeaderScreen options={classicOptions(spec)} />;
  return <ClassicScreenHeader {...spec} />;
}

/**
 * The Stack.Screen options of a top bar, for a layout that declares its screens' headers: the native bar's options (the same object until the
 * bar shows something new) where the native bar draws, else the `header` function that draws the Tamagui header.
 */
export function useScreenHeaderOptions({
  glass,
  placement: _placement,
  ...spec
}: ScreenHeaderProps): NativeStackNavigationOptions {
  const env = useHeaderEnv(glass === true);
  const native = useNativeHeaderOptions(spec, nativeHeaderActive(env));
  return native ?? classicOptions(spec);
}
