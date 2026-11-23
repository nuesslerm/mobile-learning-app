import {
  act,
  routerHarness,
  screen,
  setLiquidGlass,
  setStackHeadersRendered,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import { useGlassHeaderStore } from '@lib/tamagui-design-system';
import { Platform } from 'react-native';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { app } from './app';
import { forgetLoggedLines, loggedLines } from './loggedLines';

const realOS = Platform.OS;
const setPlatform = (os: 'web' | 'ios') => {
  (Platform as { OS: string }).OS = os;
};

function glassSetting(enabled: boolean) {
  act(() => {
    useGlassHeaderStore.setState({ enabled });
  });
}

function nativeBar() {
  setPlatform('ios');
  setLiquidGlass(true);
  setStackHeadersRendered(true);
}

beforeEach(async () => {
  glassSetting(false);
  await forgetLoggedLines();
});

afterEach(() => {
  setPlatform(realOS as 'web');
});

describe('Settings screens on the native glass bar', () => {
  it('Given the About screen on iOS 26 with the setting on, When the back item is pressed, Then the screen went back and settings.about.back was logged', async () => {
    // Given the native bar
    nativeBar();
    glassSetting(true);
    await app.render('/settings/about');
    expect(screen.queryByTestId('AboutSettingsScreenHeader')).toBeNull();

    // When the back item is pressed
    await userEvent.click(screen.getByTestId('NativeHeaderItem-Go back'));

    // Then backTo ran (the router went back) and the line was logged
    expect(routerHarness().calls()).toContainEqual(['back']);
    expect((await loggedLines('settings.about.back')).length).toBe(1);
  });

  it('Given the setting is off, When the About screen mounts, Then the Tamagui header and its ids draw and no native bar does', async () => {
    // Given the native environment with the setting off
    nativeBar();

    // When About mounts
    await app.render('/settings/about');

    // Then the header is the classic one
    expect(screen.getByTestId('AboutSettingsScreenHeader')).not.toBeNull();
    expect(screen.getByTestId('AboutSettingsScreenHeaderBack')).not.toBeNull();
    expect(screen.queryByTestId('NativeHeader')).toBeNull();
  });

  it('Given the Appearance screen on the Tamagui bar, When Glass top bars is turned On from its own row and then Off, Then the bar becomes the native one and then the Tamagui one, with the body kept', async () => {
    // Given the Appearance screen on iOS 26 with the setting off
    nativeBar();
    await app.render('/settings/appearance');
    expect(screen.getByTestId('AppearanceSettingsScreenHeader')).not.toBeNull();
    const body = screen.getByTestId('AppearanceSettingsScreen-ScrollView');

    // When On is pressed in the row
    await userEvent.click(
      screen.getByTestId('AppearanceSettingsScreen-GlassHeadersControl-on')
    );

    // Then the native bar draws, the Tamagui one is gone and the body is the same element
    await waitFor(() => screen.getByTestId('NativeHeaderItem-Go back'));
    expect(screen.queryByTestId('AppearanceSettingsScreenHeader')).toBeNull();
    expect(screen.getByTestId('AppearanceSettingsScreen-ScrollView')).toBe(
      body
    );

    // When Off is pressed
    await userEvent.click(
      screen.getByTestId('AppearanceSettingsScreen-GlassHeadersControl-off')
    );

    // Then the Tamagui bar is back
    await waitFor(() => screen.getByTestId('AppearanceSettingsScreenHeader'));
    expect(screen.queryByTestId('NativeHeader')).toBeNull();
    expect(screen.getByTestId('AppearanceSettingsScreen-ScrollView')).toBe(
      body
    );
  });
});
