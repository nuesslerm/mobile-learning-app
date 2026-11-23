import {
  act,
  routerHarness,
  screen,
  setStackHeadersRendered,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import { useNativeHeaderStore } from '@lib/tamagui-design-system';
import { Platform } from 'react-native';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { app } from './app';
import { forgetLoggedLines, loggedLines } from './loggedLines';

const realOS = Platform.OS;
const setPlatform = (os: 'web' | 'ios') => {
  (Platform as { OS: string }).OS = os;
};

function nativeSetting(enabled: boolean) {
  act(() => {
    useNativeHeaderStore.setState({ enabled });
  });
}

function nativeBar() {
  setPlatform('ios');
  setStackHeadersRendered(true);
}

beforeEach(async () => {
  nativeSetting(true);
  await forgetLoggedLines();
});

afterEach(() => {
  setPlatform(realOS as 'web');
});

describe('Settings screens on the native header', () => {
  it('Given the About screen on iOS with the setting on, When the back item is pressed, Then the screen went back and settings.about.back was logged', async () => {
    // Given the native bar
    nativeBar();
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
    nativeSetting(false);

    // When About mounts
    await app.render('/settings/about');

    // Then the header is the classic one
    expect(screen.getByTestId('AboutSettingsScreenHeader')).not.toBeNull();
    expect(screen.getByTestId('AboutSettingsScreenHeaderBack')).not.toBeNull();
    expect(screen.queryByTestId('NativeHeader')).toBeNull();
  });

  it('Given the Appearance screen on the native header, When Native headers is turned Off from its own row and then On, Then the bar becomes the Tamagui one and then the native one, with the body kept', async () => {
    // Given the Appearance screen on iOS with the setting on (the default)
    nativeBar();
    await app.render('/settings/appearance');
    await waitFor(() => screen.getByTestId('NativeHeaderItem-Go back'));
    expect(screen.queryByTestId('AppearanceSettingsScreenHeader')).toBeNull();
    const body = screen.getByTestId('AppearanceSettingsScreen-ScrollView');

    // When Off is pressed in the row
    await userEvent.click(
      screen.getByTestId('AppearanceSettingsScreen-NativeHeadersControl-off')
    );

    // Then the Tamagui bar draws, the native one is gone and the body is the same element
    await waitFor(() => screen.getByTestId('AppearanceSettingsScreenHeader'));
    expect(screen.queryByTestId('NativeHeader')).toBeNull();
    expect(screen.getByTestId('AppearanceSettingsScreen-ScrollView')).toBe(
      body
    );

    // When On is pressed
    await userEvent.click(
      screen.getByTestId('AppearanceSettingsScreen-NativeHeadersControl-on')
    );

    // Then the native bar is back
    await waitFor(() => screen.getByTestId('NativeHeaderItem-Go back'));
    expect(screen.queryByTestId('AppearanceSettingsScreenHeader')).toBeNull();
    expect(screen.getByTestId('AppearanceSettingsScreen-ScrollView')).toBe(
      body
    );
  });
});
