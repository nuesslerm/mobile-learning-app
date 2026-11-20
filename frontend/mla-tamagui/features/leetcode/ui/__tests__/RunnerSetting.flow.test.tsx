import { screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { afterEach, describe, expect, it } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { useRunnerSettingStore } from '../../state/runnerSettingStore';

afterEach(() => useRunnerSettingStore.setState({ runner: 'backend' }));

describe('the runner setting on the problem list', () => {
  it('given the list, when the person picks This device and then Server, then the setting follows and picking the chosen one again keeps it', async () => {
    // Given the problem list with the default runner
    await app.render('/');
    expect(screen.getByTestId('LeetcodeListRunnerRow')).toBeTruthy();
    expect(useRunnerSettingStore.getState().runner).toBe('backend');

    // When they pick This device
    await userEvent.click(screen.getByTestId('LeetcodeListRunnerDevice'));

    // Then the setting is the device, and tapping it again does not clear it
    await waitFor(() => expect(useRunnerSettingStore.getState().runner).toBe('device'));
    await userEvent.click(screen.getByTestId('LeetcodeListRunnerDevice'));
    expect(useRunnerSettingStore.getState().runner).toBe('device');

    // When they pick Server again
    await userEvent.click(screen.getByTestId('LeetcodeListRunnerBackend'));
    await waitFor(() => expect(useRunnerSettingStore.getState().runner).toBe('backend'));
  });
});
