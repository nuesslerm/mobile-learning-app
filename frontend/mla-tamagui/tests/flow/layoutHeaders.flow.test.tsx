import {
  act,
  routerHarness,
  screen,
  setStackHeadersRendered,
  userEvent,
  waitFor,
  within,
} from '@lib/flow-test-harness';
import { useNativeHeaderStore } from '@lib/tamagui-design-system';
import { Platform } from 'react-native';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { app } from './app';

const realOS = Platform.OS;

beforeEach(() => {
  act(() => {
    useNativeHeaderStore.setState({ enabled: true });
  });
});

afterEach(() => {
  (Platform as { OS: string }).OS = realOS;
});

const layouts = [
  ['/leetcode/login', 'LeetCodeLoginHeader', 'Sign in'],
  ['/leetcode/toolbar', 'ToolbarConfigHeader', 'Toolbar'],
  ['/service-report', 'ServiceReportHeader', 'Service Report'],
] as const;

describe('Layout headers', () => {
  it.each(layouts)(
    'Given %s on web, When it mounts, Then the Tamagui header draws with its ids and Back goes back',
    async (route, testID) => {
      // Given the classic environment with the stack's headers rendered
      setStackHeadersRendered(true);
      await app.render(route);

      // When Back is pressed
      await userEvent.click(screen.getByTestId(`${testID}Back`));

      // Then the router went back and no native bar drew
      expect(routerHarness().calls()).toContainEqual(['back']);
      expect(screen.queryByTestId('NativeHeader')).toBeNull();
    }
  );

  it.each(layouts)(
    'Given %s on iOS with the setting on, When it mounts, Then the twin shows the back item and the title and no classic header',
    async (route, testID, title) => {
      // Given the native bar
      (Platform as { OS: string }).OS = 'ios';
      setStackHeadersRendered(true);

      // When the screen mounts
      await app.render(route);

      // Then the back item and the title are native and the classic header is absent
      await waitFor(() => screen.getByTestId('NativeHeaderItem-Go back'));
      expect(
        within(screen.getByTestId('ScreenHeaderGlassTitle')).getByText(title)
      ).not.toBeNull();
      expect(screen.queryByTestId(testID)).toBeNull();

      // When the back item is pressed
      await userEvent.click(screen.getByTestId('NativeHeaderItem-Go back'));

      // Then the router went back
      expect(routerHarness().calls()).toContainEqual(['back']);
    }
  );
});
