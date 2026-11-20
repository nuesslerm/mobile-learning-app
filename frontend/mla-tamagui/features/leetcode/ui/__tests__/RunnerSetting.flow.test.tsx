import { screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { afterEach, describe, expect, it } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';

afterEach(() => useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER }));

describe('the runner setting on the problem list', () => {
  it('given the list, when the person picks Server and then This device, then the setting follows and picking the chosen one again keeps it', async () => {
    // Given the problem list with the default runner, this device
    await app.render('/');
    expect(screen.getByTestId('LeetcodeListRunnerRow')).toBeTruthy();
    expect(useRunnerSettingStore.getState().runner).toBe('device');

    // When they pick Server
    await userEvent.click(screen.getByTestId('LeetcodeListRunnerBackend'));

    // Then the setting is the backend, and tapping it again does not clear it
    await waitFor(() => expect(useRunnerSettingStore.getState().runner).toBe('backend'));
    await userEvent.click(screen.getByTestId('LeetcodeListRunnerBackend'));
    expect(useRunnerSettingStore.getState().runner).toBe('backend');

    // When they pick This device again
    await userEvent.click(screen.getByTestId('LeetcodeListRunnerDevice'));
    await waitFor(() => expect(useRunnerSettingStore.getState().runner).toBe('device'));
  });
});
