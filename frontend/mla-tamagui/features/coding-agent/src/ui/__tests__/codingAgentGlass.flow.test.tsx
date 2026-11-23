import { createFixturePorts } from '@feature/coding-agent/testing';
import {
  routerHarness,
  screen,
  setStackHeadersRendered,
  userEvent,
  waitFor,
} from '@lib/flow-test-harness';
import { Platform } from 'react-native';
import { afterEach, describe, expect, it } from 'vitest';

import { app } from '../../__tests__/flowApp';

const FX_THREAD_BASIC = 'fx-thread-fx-basic';
const realOS = Platform.OS;

afterEach(() => {
  (Platform as { OS: string }).OS = realOS;
});

function render(route: string) {
  (Platform as { OS: string }).OS = 'ios';
  setStackHeadersRendered(true);
  return app.render(route, {
    overrides: { 'coding-agent': createFixturePorts() },
    search: { transport: 'fixture' },
  });
}

describe('Coding agent on the native header', () => {
  it('Given the thread list on the native bar, When it mounts, Then the New thread action is native, there is no back item and the classic header is absent', async () => {
    // Given the native bar
    await render('/coding-agent');

    // Then the New thread action is a native item and nothing else leads the bar
    await waitFor(() => screen.getByTestId('NativeHeaderItem-New thread'));
    expect(screen.queryByTestId('NativeHeaderItem-Go back')).toBeNull();
    expect(screen.queryByTestId('ThreadListHeader')).toBeNull();
    expect(screen.queryByTestId('NewThreadButton')).toBeNull();
  });

  it('Given a thread on the native bar, When the settings action is pressed, Then the thread-settings sheet opens', async () => {
    // Given the thread screen
    await render(`/coding-agent/${FX_THREAD_BASIC}`);
    await waitFor(() => screen.getByTestId('NativeHeaderItem-Thread settings'));
    expect(screen.queryByTestId('ThreadScreenHeader')).toBeNull();

    // When the settings action is pressed
    await userEvent.click(screen.getByTestId('NativeHeaderItem-Thread settings'));

    // Then the thread-settings sheet opened
    await waitFor(
      () => expect(screen.getByTestId('ThreadSettingsSheet')).toBeTruthy(),
      { timeout: 12_000 }
    );
  });

  it('Given a dispatch run on the native bar, When the back item is pressed, Then the router went back', async () => {
    // Given the run screen
    await render('/coding-agent/runs/fx-run-finished');
    await waitFor(() => screen.getByTestId('NativeHeaderItem-Go back'));
    expect(screen.queryByTestId('DispatchRunHeader')).toBeNull();

    // When the back item is pressed
    await userEvent.click(screen.getByTestId('NativeHeaderItem-Go back'));

    // Then the router went back
    expect(routerHarness().calls()).toContainEqual(['back']);
  });
});
