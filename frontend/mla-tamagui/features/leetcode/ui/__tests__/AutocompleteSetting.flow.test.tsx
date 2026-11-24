import { screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { DEFAULT_AUTOCOMPLETE, useAutocompleteSettingStore } from '../../state/autocompleteSettingStore';

// reset before the screen mounts: a store write while a screen is still mounted is an update outside act
beforeEach(() => useAutocompleteSettingStore.setState({ autocomplete: DEFAULT_AUTOCOMPLETE }));

describe('the suggestions setting on the problem list', () => {
  it('given suggestions off, when the person turns them on and taps On again, then the setting is on and stays on', async () => {
    useAutocompleteSettingStore.setState({ autocomplete: false });
    await app.render('/');
    expect(screen.getByTestId('LeetcodeListAutocompleteRow')).toBeTruthy();

    await userEvent.click(screen.getByTestId('LeetcodeListAutocompleteOn'));
    await waitFor(() => expect(useAutocompleteSettingStore.getState().autocomplete).toBe(true));
    await userEvent.click(screen.getByTestId('LeetcodeListAutocompleteOn'));

    expect(useAutocompleteSettingStore.getState().autocomplete).toBe(true);
  });

  it('given suggestions on, when the person taps Off, then the setting is off', async () => {
    useAutocompleteSettingStore.setState({ autocomplete: true });
    await app.render('/');

    await userEvent.click(screen.getByTestId('LeetcodeListAutocompleteOff'));

    await waitFor(() => expect(useAutocompleteSettingStore.getState().autocomplete).toBe(false));
  });
});
