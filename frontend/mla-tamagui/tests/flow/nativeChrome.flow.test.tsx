import {
  act,
  navigationTheme,
  renderHook,
  setLiquidGlass,
} from '@lib/flow-test-harness';
import {
  useGlassHeaderStore,
  usePresetThemeStore,
} from '@lib/tamagui-design-system';
import { DarkTheme, DefaultTheme } from 'expo-router';
import { Appearance, Platform } from 'react-native';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import {
  useNativeChromeSync,
  useNavigationTheme,
} from '../../app-shell/NativeChromeBridge';

import { appWithRoot } from './appWithRoot';

const realOS = Platform.OS;
const setPlatform = (os: 'web' | 'ios' | 'android') => {
  (Platform as { OS: string }).OS = os;
};

// react-native-web's Appearance has no setColorScheme, so a recorder stands where the native module is.
type Chrome = { setColorScheme?: (scheme: 'light' | 'dark' | 'auto') => void };
const chrome = Appearance as unknown as Chrome;
let recorded: ('light' | 'dark' | 'auto')[] = [];

function settings(values: {
  glass?: boolean;
  mode?: 'system' | 'light' | 'dark';
}) {
  act(() => {
    if (values.glass !== undefined)
      useGlassHeaderStore.setState({ enabled: values.glass });
    if (values.mode !== undefined)
      usePresetThemeStore.setState({ themeMode: values.mode });
  });
}

function nativePath() {
  setPlatform('ios');
  setLiquidGlass(true);
}

beforeEach(() => {
  recorded = [];
  chrome.setColorScheme = (scheme) => {
    recorded.push(scheme);
  };
  settings({ glass: false, mode: 'system' });
});

afterEach(() => {
  setPlatform(realOS as 'web');
});

describe('the OS chrome follows the app only while the native bar is in play', () => {
  it('Given iOS with glass, the setting on and Dark chosen, When the bridge mounts, Then the OS was told dark', () => {
    // Given the native path with the person's choice Dark
    nativePath();
    settings({ glass: true, mode: 'dark' });

    // When the bridge mounts
    renderHook(() => useNativeChromeSync());

    // Then the OS was told dark
    expect(recorded).toEqual(['dark']);
  });

  it('Given the native path and System chosen, When the bridge mounts, Then the OS was handed its own decision back', () => {
    // Given the native path with System chosen
    nativePath();
    settings({ glass: true, mode: 'system' });

    // When the bridge mounts
    renderHook(() => useNativeChromeSync());

    // Then auto was recorded: the OS decides
    expect(recorded).toEqual(['auto']);
  });

  it('Given the bridge mounted on the native path, When the choice flips to Light, Then it records again', () => {
    // Given the bridge on Dark
    nativePath();
    settings({ glass: true, mode: 'dark' });
    renderHook(() => useNativeChromeSync());

    // When Light is chosen
    settings({ mode: 'light' });

    // Then the OS was told light after dark
    expect(recorded).toEqual(['dark', 'light']);
  });

  it('Given the bridge mounted on the native path, When the setting is turned off, Then the OS is given its decision back', () => {
    // Given the bridge on Dark
    nativePath();
    settings({ glass: true, mode: 'dark' });
    renderHook(() => useNativeChromeSync());

    // When the setting is turned off
    settings({ glass: false });

    // Then auto was recorded last
    expect(recorded).toEqual(['dark', 'auto']);
  });

  it.each([
    ['the setting is off', () => nativePath()],
    [
      'the platform is android',
      () => {
        setPlatform('android');
        setLiquidGlass(true);
        settings({ glass: true });
      },
    ],
    [
      'the platform is web',
      () => {
        setPlatform('web');
        setLiquidGlass(true);
        settings({ glass: true });
      },
    ],
    [
      'Liquid Glass is not available',
      () => {
        setPlatform('ios');
        settings({ glass: true });
      },
    ],
  ])(
    'Given %s, When the bridge mounts and the choice changes, Then the OS is never told',
    (_name, spoil) => {
      // Given a path that is not the native one
      spoil();
      settings({ mode: 'dark' });

      // When the bridge mounts and the choice changes
      renderHook(() => useNativeChromeSync());
      settings({ mode: 'light' });

      // Then nothing was recorded
      expect(recorded).toEqual([]);
    }
  );
});

describe('the navigator theme', () => {
  it('Given the native path and Dark chosen, When the theme is read, Then it is react-navigation dark; and off the native path it is the light one', () => {
    // Given the native path with Dark
    nativePath();
    settings({ glass: true, mode: 'dark' });

    // When the theme is read
    const { result } = renderHook(() => useNavigationTheme());

    // Then dark
    expect(result.current).toBe(DarkTheme);

    // When the setting is turned off
    settings({ glass: false });

    // Then the light theme the navigator has today
    expect(result.current).toBe(DefaultTheme);
  });

  it('Given the web build with Dark chosen, When the root layout renders, Then the navigator theme is the light one, as it is today', async () => {
    // Given Dark chosen on web
    settings({ mode: 'dark' });

    // When the root renders
    await appWithRoot.render('/');

    // Then the ThemeProvider got react-navigation's default
    expect(navigationTheme()).toBe(DefaultTheme);
  });
});
