import { Stack, type NativeStackNavigationOptions } from 'expo-router';
import {
  useContext,
  useEffect,
  useMemo,
  useRef,
  type ReactElement,
} from 'react';
import { useColorScheme, useWindowDimensions, View } from 'react-native';
import { SafeAreaInsetsContext } from 'react-native-safe-area-context';

import { useHeaderHeightStore } from '../../lib/headerHeightStore';
import { useTheme, XStack } from '../../primitives';
import { usePresetThemeStore } from '../../theme/state/presetThemeStore';
import { Icon } from '../../ui/icon';
import { Text } from '../../ui/text';
import { Spinner } from '../../ui/spinner';
import {
  ClassicScreenHeader,
  DEFAULT_TOP_INSET,
} from '../classic-screen-header';

import { MenuPanel } from '../menu-panel';

import { GlassTitle } from './glass-title';
import type { HeaderEnv } from './header-env';
import { HEADER_BUSY_LABEL, type ScreenHeaderSpec } from './header-spec';
import {
  buildScreenHeaderOptions,
  headerSignature,
  NATIVE_BAR_CONTENT_HEIGHT,
  type HeaderRenderers,
} from './screen-header-options';
import { collectHandlers, withStableHandlers } from './stable-spec';

// Only built when the environment already said native, so the options builder's own check has nothing left to decide.
const NATIVE_ENV: HeaderEnv = {
  platform: 'ios',
  nativeHeaders: true,
  optedIn: true,
};

const RENDERERS: HeaderRenderers = {
  classic: (spec) => <ClassicScreenHeader {...spec} />,
  busy: (size): ReactElement => (
    <View
      style={{
        width: size,
        height: size,
        alignItems: 'center',
        justifyContent: 'center',
      }}
    >
      <Spinner accessibilityLabel={HEADER_BUSY_LABEL} />
    </View>
  ),
  title: (props) => <GlassTitle {...props} />,
  status: ({ icon, label, testID, menu }) => {
    const content = (
      <XStack items="center" gap="$1" testID={menu ? undefined : testID}>
        <Icon as={icon} size={13} color="$color10" />
        <Text color="$color10" fontSize={11} lineHeight={14}>{label}</Text>
      </XStack>
    );
    return menu
      ? <MenuPanel {...menu} icon={icon} testID={testID} size={38} trigger={content} />
      : content;
  },
};

/** The spec's handlers go through one ref, so the bar (and an open UIMenu) is rebuilt only when it shows something new, never because a render made new closures. */
function useStableSpec(
  spec: ScreenHeaderSpec,
  active: boolean
): ScreenHeaderSpec {
  const latest = useRef(spec);
  latest.current = spec;
  const held = useRef<{ signature: string; stable: ScreenHeaderSpec } | null>(
    null
  );
  if (!active) return spec;
  const signature = headerSignature(spec);
  if (held.current?.signature !== signature)
    held.current = {
      signature,
      stable: withStableHandlers(spec, (key) =>
        collectHandlers(latest.current).get(key)?.()
      ),
    };
  return held.current.stable;
}

/** The Stack.Screen options of the native glass bar, the same object until the bar shows something new; undefined while the native bar is not the one to draw. */
export function useNativeHeaderOptions(
  spec: ScreenHeaderSpec,
  active: boolean
): NativeStackNavigationOptions | undefined {
  const stable = useStableSpec(spec, active);
  const theme = useTheme();
  const { width } = useWindowDimensions();
  const systemScheme = useColorScheme();
  const themeMode = usePresetThemeStore((state) => state.themeMode);
  const scheme =
    themeMode === 'system'
      ? systemScheme === 'dark'
        ? 'dark'
        : 'light'
      : themeMode;
  const background = String(theme.background?.val);
  const tint = String(theme.color?.val);

  // The toast host sits clear of the bar by this: the native bar reports no height, so it is the inset plus its content height.
  const setHeaderHeight = useHeaderHeightStore((state) => state.setHeight);
  const insets = useContext(SafeAreaInsetsContext);
  const top = insets?.top ?? DEFAULT_TOP_INSET;
  useEffect(() => {
    if (active) setHeaderHeight(top + NATIVE_BAR_CONTENT_HEIGHT);
  }, [active, top, setHeaderHeight]);

  return useMemo(
    () =>
      active
        ? buildScreenHeaderOptions({
            spec: stable,
            env: NATIVE_ENV,
            scheme,
            theme: { background, tint },
            windowWidth: width,
            renderers: RENDERERS,
          })
        : undefined,
    [active, stable, scheme, background, tint, width]
  );
}

/** Declares a screen's top bar to its navigator; nothing renders in the screen's own body. */
export function HeaderScreen({
  options,
}: {
  options: NativeStackNavigationOptions;
}) {
  return <Stack.Screen options={options} />;
}
