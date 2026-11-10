import { usePresetThemeStore } from '@lib/tamagui-design-system';
import { beforeEach, describe, expect, it } from 'vitest';

import { appWithRoot } from './appWithRoot';

const themeClass = (name: string): HTMLElement | null =>
  document.querySelector(`.t_${name}`);

describe('RootLayout theme flow (preset theme store)', () => {
  beforeEach(() => {
    usePresetThemeStore.setState({ themeMode: 'system', activeThemeId: null });
  });

  it('given a preset set in the store, when the root layout renders, then the resolved theme name is that preset\u2019s, not the raw device scheme', async () => {
    usePresetThemeStore.setState({ themeMode: 'dark', activeThemeId: 'forest-fresh' });

    await appWithRoot.render('/');

    expect(themeClass('forest-fresh_dark')).toBeTruthy();
    expect(themeClass('forest-fresh_light')).toBeNull();
  });

  it('given the store reset to system mode, when the root layout renders, then the theme falls back to the default preset following the device scheme', async () => {
    await appWithRoot.render('/');

    expect(themeClass('ios-native_light')).toBeTruthy();
  });
});