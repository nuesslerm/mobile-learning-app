import { screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { DEFAULT_AUTOCOMPLETE, useAutocompleteSettingStore } from '../../state/autocompleteSettingStore';

// reset before the screen mounts: a store write while a screen is still mounted is an update outside act
beforeEach(() => useAutocompleteSettingStore.setState({ autocomplete: DEFAULT_AUTOCOMPLETE }));

describe('the suggestions setting on the problem list', () => {
  it('given the list, when the person turns suggestions on and taps On again, then the setting is on and stays on', async () => {
    await app.render('/');
    expect(screen.getByTestId('LeetcodeListAutocompleteRow')).toBeTruthy();
    expect(useAutocompleteSettingStore.getState().autocomplete).toBe(false);

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
