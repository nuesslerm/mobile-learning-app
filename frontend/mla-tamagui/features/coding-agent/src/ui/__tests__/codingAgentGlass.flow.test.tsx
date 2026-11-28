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
