import { screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { DEFAULT_CONSOLE, useConsoleSettingStore } from '../../state/consoleSettingStore';

// reset before the screen mounts: a store write while a screen is still mounted is an update outside act
beforeEach(() => useConsoleSettingStore.setState({ enabled: DEFAULT_CONSOLE }));

describe('the console setting on the problem list', () => {
  it('given the list, when the person turns the console on and taps On again, then the setting is on and stays on', async () => {
    await app.render('/');
    expect(screen.getByTestId('LeetcodeListConsoleRow')).toBeTruthy();
    expect(useConsoleSettingStore.getState().enabled).toBe(false);

    await userEvent.click(screen.getByTestId('LeetcodeListConsoleOn'));
    await waitFor(() => expect(useConsoleSettingStore.getState().enabled).toBe(true));
    await userEvent.click(screen.getByTestId('LeetcodeListConsoleOn'));

    expect(useConsoleSettingStore.getState().enabled).toBe(true);
  });

  it('given the console on, when the person taps Off, then the setting is off', async () => {
    useConsoleSettingStore.setState({ enabled: true });
    await app.render('/');

    await userEvent.click(screen.getByTestId('LeetcodeListConsoleOff'));

    await waitFor(() => expect(useConsoleSettingStore.getState().enabled).toBe(false));
  });
});
