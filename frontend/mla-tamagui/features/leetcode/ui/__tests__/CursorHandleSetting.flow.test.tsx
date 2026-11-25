import { screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../tests/flow/loggedLines';
import { DEFAULT_CURSOR_HANDLE, useCursorHandleSettingStore } from '../../state/editor/cursorHandleSettingStore';

// reset before the screen mounts: a store write while a screen is still mounted is an update outside act
beforeEach(() => {
  process.env.EXPO_PUBLIC_LEETCODE_CURSOR_PAD = '1';
});
afterEach(() => {
  delete process.env.EXPO_PUBLIC_LEETCODE_CURSOR_PAD;
});

beforeEach(() => useCursorHandleSettingStore.setState({ cursorHandle: DEFAULT_CURSOR_HANDLE }));

describe('the cursor handle setting on the problem list', () => {
  it('given the list, when the person taps On, then the tap is logged, the setting is on, and it stays on after a second tap', async () => {
    await forgetLoggedLines();
    await app.render('/blind75');
    expect(screen.getByTestId('LeetcodeListCursorHandleRow')).toBeTruthy();
    expect(useCursorHandleSettingStore.getState().cursorHandle).toBe(false);

    await userEvent.click(screen.getByTestId('LeetcodeListCursorHandleOn'));
    await waitFor(() => expect(useCursorHandleSettingStore.getState().cursorHandle).toBe(true));
    await userEvent.click(screen.getByTestId('LeetcodeListCursorHandleOn'));

    expect(useCursorHandleSettingStore.getState().cursorHandle).toBe(true);
    const taps = await loggedLines('leetcode.list.cursor_handle_tap');
    expect(taps[0]?.context).toMatchObject({ cursorHandle: true });
  });

  it('given the handle on, when the person taps Off, then the setting is off again', async () => {
    useCursorHandleSettingStore.setState({ cursorHandle: true });
    await app.render('/blind75');

    await userEvent.click(screen.getByTestId('LeetcodeListCursorHandleOff'));

    await waitFor(() => expect(useCursorHandleSettingStore.getState().cursorHandle).toBe(false));
  });

  it('given the list, when it is shown, then the row reads Off as the choice made until the person turns the handle on', async () => {
    await app.render('/blind75');

    expect(screen.getByTestId('LeetcodeListCursorHandleOff').getAttribute('aria-checked')).toBe('true');
    expect(screen.getByTestId('LeetcodeListCursorHandleOn').getAttribute('aria-checked')).toBe('false');
  });
});
