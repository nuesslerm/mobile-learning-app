import { screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../tests/flow/loggedLines';
import { DEFAULT_BLOCK_VIEW, useBlockViewSettingStore } from '../../state/blockViewSettingStore';

// reset before the screen mounts: a store write while a screen is still mounted is an update outside act
beforeEach(() => useBlockViewSettingStore.setState({ blockView: DEFAULT_BLOCK_VIEW }));

describe('the block view setting on the problem list', () => {
  it('given the list, when the person taps On, then the tap is logged, the setting is on, and it stays on after a second tap', async () => {
    await forgetLoggedLines();
    await app.render('/');
    expect(screen.getByTestId('LeetcodeListBlockViewRow')).toBeTruthy();
    expect(useBlockViewSettingStore.getState().blockView).toBe(false);

    await userEvent.click(screen.getByTestId('LeetcodeListBlockViewOn'));
    await waitFor(() => expect(useBlockViewSettingStore.getState().blockView).toBe(true));
    await userEvent.click(screen.getByTestId('LeetcodeListBlockViewOn'));

    expect(useBlockViewSettingStore.getState().blockView).toBe(true);
    const taps = await loggedLines('leetcode.list.block_view_tap');
    expect(taps[0]?.context).toMatchObject({ blockView: true });
  });

  it('given the block view on, when the person taps Off, then the setting is off again', async () => {
    useBlockViewSettingStore.setState({ blockView: true });
    await app.render('/');

    await userEvent.click(screen.getByTestId('LeetcodeListBlockViewOff'));

    await waitFor(() => expect(useBlockViewSettingStore.getState().blockView).toBe(false));
  });

  it('given the list, when it is shown, then the row reads Off as the choice made and is labelled as experimental', async () => {
    await app.render('/');

    expect(screen.getByTestId('LeetcodeListBlockViewOff').getAttribute('aria-checked')).toBe('true');
    expect(screen.getByTestId('LeetcodeListBlockViewOn').getAttribute('aria-checked')).toBe('false');
    expect(screen.getByTestId('LeetcodeListBlockViewLabel').textContent).toBe('Blocks (experimental)');
  });
});
