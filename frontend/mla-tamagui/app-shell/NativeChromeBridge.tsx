import {
  useNativeHeaderPath,
  usePresetThemeStore,
} from '@lib/tamagui-design-system';
import { DarkTheme, DefaultTheme, type Theme } from 'expo-router';
import { useEffect } from 'react';
import { Appearance, Platform, useColorScheme } from 'react-native';

import { chromeSchemeFor, navigationThemeFor } from './nativeChromeTheme';

export function useResolvedScheme(): 'light' | 'dark' {
  const themeMode = usePresetThemeStore((state) => state.themeMode);
  const systemScheme = useColorScheme();
  if (themeMode === 'system') return systemScheme === 'dark' ? 'dark' : 'light';
  return themeMode;
}

/** The value for the navigator's ThemeProvider. */
export function useNavigationTheme(): Theme {
  const active = useNativeHeaderPath();
  const scheme = useResolvedScheme();
  return navigationThemeFor(active, scheme, {
    light: DefaultTheme,
    dark: DarkTheme,
  });
}

/**
 * Keyboards, alerts and the native bar follow the OS scheme, not the app's choice. With the native bar on iOS the OS is told the person's
 * choice, and given the decision back ('auto') when the native path turns off. Never called on Android or web.
 */
export function useNativeChromeSync(): void {
  const applies = useNativeHeaderPath() && Platform.OS === 'ios';
  const themeMode = usePresetThemeStore((state) => state.themeMode);
  useEffect(() => {
    if (applies) Appearance.setColorScheme(chromeSchemeFor(themeMode));
  }, [applies, themeMode]);
  useEffect(() => {
    if (!applies) return undefined;
    return () => Appearance.setColorScheme('auto');
  }, [applies]);
}

export function NativeChromeBridge() {
  useNativeChromeSync();
  return null;
}
