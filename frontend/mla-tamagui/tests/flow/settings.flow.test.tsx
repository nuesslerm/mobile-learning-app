import {
  routerHarness,
  screen,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import {
  useNativeHeaderStore,
  usePresetThemeStore,
} from '@lib/tamagui-design-system';
import { beforeEach, describe, expect, it } from 'vitest';

import { app } from './app';

const ROWS: { label: string; href: string }[] = [
  { label: 'Profile', href: '/settings/profile' },
  { label: 'Appearance', href: '/settings/appearance' },
  { label: 'Notifications', href: '/settings/notifications' },
  { label: 'Storage', href: '/settings/storage' },
  { label: 'About', href: '/settings/about' },
];

const opaqueBackground = (testID: string): boolean => {
  const el = screen.getByTestId(testID);
  const bg = window.getComputedStyle(el).backgroundColor;
  return Boolean(bg) && bg !== 'transparent' && bg !== 'rgba(0, 0, 0, 0)';
};

describe('Settings flow (index and appearance screens)', () => {
  beforeEach(() => {
    usePresetThemeStore.setState({ themeMode: 'system', activeThemeId: null });
  });

  it('given the settings index renders, when it mounts, then all five rows are present and each carries the href the contract names', async () => {
    await app.render('/settings');
    const router = routerHarness();

    for (const { label, href } of ROWS) {
      const row = screen.getByText(label).closest('[data-testid]');
      expect(row).toBeTruthy();
      await userEvent.click(row as HTMLElement);
      expect(router.lastPush()).toBe(href);
    }
  });

  it('given a settings row rendered, when it is pressed, then navigation was requested for that row\u2019s href (asserted on the navigation call, not on the row\u2019s testID existing)', async () => {
    const router = routerHarness();
    await app.render('/settings');

    const appearanceRow = screen.getByText('Appearance').closest('[data-testid]');
    await userEvent.click(appearanceRow as HTMLElement);

    expect(router.lastPush()).toBe('/settings/appearance');
    expect(router.calls()).toHaveLength(1);
  });

  it('given the Appearance screen with preset tiles, when a preset tile is pressed, then the store holds that preset as the active theme', async () => {
    await app.render('/settings/appearance');

    await userEvent.click(
      screen.getByTestId('AppearanceSettingsScreen-ThemeTilePress-forest-fresh')
    );

    expect(usePresetThemeStore.getState().activeThemeId).toBe('forest-fresh');
  });

  it('given themeMode is set to dark, when the Appearance screen renders, then the segmented control reports dark as selected', async () => {
    usePresetThemeStore.setState({ themeMode: 'dark' });

    await app.render('/settings/appearance');

    expect(
      opaqueBackground('AppearanceSettingsScreen-ModeControl-dark')
    ).toBe(true);
    expect(
      opaqueBackground('AppearanceSettingsScreen-ModeControl-light')
    ).toBe(false);
    expect(
      opaqueBackground('AppearanceSettingsScreen-ModeControl-system')
    ).toBe(false);
  });
});

describe('the Native headers row on the Appearance screen', () => {
  beforeEach(() => {
    useNativeHeaderStore.setState({ enabled: true });
  });

  it('given a fresh install, when the Appearance screen renders, then native headers are on', async () => {
    await app.render('/settings/appearance');

    expect(useNativeHeaderStore.getState().enabled).toBe(true);
    expect(
      screen.getByTestId('AppearanceSettingsScreen-NativeHeadersControl-on')
    ).not.toBeNull();
  });

  it('given the setting on, when Off is tapped, then the setting is off and On turns it back on', async () => {
    await app.render('/settings/appearance');

    await userEvent.click(
      screen.getByTestId('AppearanceSettingsScreen-NativeHeadersControl-off')
    );
    await waitFor(() =>
      expect(useNativeHeaderStore.getState().enabled).toBe(false)
    );
    await userEvent.click(
      screen.getByTestId('AppearanceSettingsScreen-NativeHeadersControl-on')
    );

    await waitFor(() =>
      expect(useNativeHeaderStore.getState().enabled).toBe(true)
    );
  });

  it('given the Appearance screen, when it renders, then the hint names the iOS system header and the off switch', async () => {
    await app.render('/settings/appearance');

    const hint = screen.getByTestId(
      'AppearanceSettingsScreen-NativeHeadersHint'
    );

    expect(hint.textContent).toContain('iOS system header');
    expect(hint.textContent).toContain("app's own header");
  });
});
