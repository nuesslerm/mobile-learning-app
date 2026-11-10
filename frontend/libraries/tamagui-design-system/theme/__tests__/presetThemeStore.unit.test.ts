import { beforeEach, describe, expect, it } from 'vitest';

import { keyValueStorage } from '../../lib/keyValueStore';
import {
  resolveActiveThemeName,
  usePresetThemeStore,
} from '../state/presetThemeStore';

const STORE_KEY = 'preset-theme-store';

beforeEach(async () => {
  usePresetThemeStore.setState({ themeMode: 'system', activeThemeId: null });
  await keyValueStorage.removeItem(STORE_KEY);
});

describe('given the preset theme store with no selection', () => {
  it('when a preset is selected, then the active id and resolved Tamagui theme name reflect it', () => {
    usePresetThemeStore.getState().setActiveTheme('ocean-breeze');

    expect(usePresetThemeStore.getState().activeThemeId).toBe('ocean-breeze');
    expect(
      resolveActiveThemeName(
        usePresetThemeStore.getState().activeThemeId,
        'light',
        'light'
      )
    ).toBe('ocean-breeze_light');
  });

  it('when themeMode is system, then the OS scheme picks the light or dark variant', () => {
    expect(resolveActiveThemeName('forest-fresh', 'system', 'light')).toBe(
      'forest-fresh_light'
    );
    expect(resolveActiveThemeName('forest-fresh', 'system', 'dark')).toBe(
      'forest-fresh_dark'
    );
  });
});

describe('given the preset theme store persistence', () => {
  it('when a preset is selected, then the state is written under the store key', async () => {
    usePresetThemeStore.getState().setActiveTheme('midnight-neon');
    usePresetThemeStore.getState().setThemeMode('dark');

    const raw = await keyValueStorage.getItem(STORE_KEY);
    expect(raw).not.toBeNull();
    const persisted = JSON.parse(raw as string);
    expect(persisted.state.activeThemeId).toBe('midnight-neon');
    expect(persisted.state.themeMode).toBe('dark');
  });

  it('when the app rehydrates from persisted state, then the selected preset survives', async () => {
    await keyValueStorage.setItem(
      STORE_KEY,
      JSON.stringify({
        state: { themeMode: 'light', activeThemeId: 'desert-dawn' },
        version: 0,
      })
    );

    await usePresetThemeStore.persist.rehydrate();

    expect(usePresetThemeStore.getState().activeThemeId).toBe('desert-dawn');
    expect(usePresetThemeStore.getState().themeMode).toBe('light');
  });

  it('when the persisted state is corrupt, then the store falls back to its defaults', async () => {
    await keyValueStorage.setItem(STORE_KEY, '"not-an-object"');

    await usePresetThemeStore.persist.rehydrate();

    expect(usePresetThemeStore.getState().activeThemeId).toBeNull();
    expect(usePresetThemeStore.getState().themeMode).toBe('system');
  });
});

describe('given a previously selected preset', () => {
  it('when the selection is cleared, then the ios-native default theme resolves', () => {
    usePresetThemeStore.getState().setActiveTheme('royal-purple');
    usePresetThemeStore.getState().setActiveTheme(null);

    expect(usePresetThemeStore.getState().activeThemeId).toBeNull();
    expect(resolveActiveThemeName(null, 'system', 'dark')).toBe(
      'ios-native_dark'
    );
  });

  it('when an unknown id is persisted, then the ios-native default theme resolves', () => {
    expect(resolveActiveThemeName('no-such-preset', 'light', 'light')).toBe(
      'ios-native_light'
    );
  });
});
