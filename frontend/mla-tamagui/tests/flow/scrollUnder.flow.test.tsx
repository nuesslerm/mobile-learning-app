import {
  act,
  setStackHeadersRendered,
  screen,
  waitFor,
} from '@lib/flow-test-harness';
import { useNativeHeaderStore } from '@lib/tamagui-design-system';
import { Platform } from 'react-native';
import { afterEach, describe, expect, it } from 'vitest';

import { app } from './app';

const realOS = Platform.OS;
const setPlatform = (os: 'web' | 'ios') => {
  (Platform as { OS: string }).OS = os;
};

afterEach(() => {
  setPlatform(realOS as 'web');
});

const lists = [
  ['/settings', 'SettingsIndexScreen'],
  ['/settings/appearance', 'AppearanceSettingsScreen'],
  ['/dashboards', 'DashboardsIndexScreen'],
] as const;

describe('Lists that scroll under the top bar', () => {
  it.each(lists)(
    'Given %s on the native bar, When it mounts, Then the bar is transparent and the classic header is gone',
    async (route, id) => {
      // Given the native bar
      act(() => {
        useNativeHeaderStore.setState({ enabled: true });
      });
      setPlatform('ios');
      setStackHeadersRendered(true);

      // When the screen mounts
      await app.render(route);

      // Then the bar is transparent
      await waitFor(() => screen.getByTestId('NativeHeader'));
      expect(
        screen.getByTestId('NativeHeader').getAttribute('data-header-transparent')
      ).toBe('true');
      expect(screen.queryByTestId(`${id}Header`)).toBeNull();
    }
  );

  it.each(lists)(
    'Given %s on web, When it mounts, Then the classic header draws and there is no native bar',
    async (route, id) => {
      // Given web
      setPlatform('web');

      // When the screen mounts
      await app.render(route);

      // Then the Tamagui header is the bar
      await waitFor(() => screen.getByTestId(`${id}Header`));
      expect(screen.queryByTestId('NativeHeader')).toBeNull();
    }
  );
});
