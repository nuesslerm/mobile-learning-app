import { createFixturePorts } from '@feature/coding-agent/testing';
import { act, screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { afterEach, describe, expect, it } from 'vitest';

import { app } from '../../__tests__/legacyFlowApp';

const KEYBOARD_HEIGHT = 300;
const FX_THREAD_BASIC = 'fx-thread-fx-basic';

// the keyboard up, signalled the way the device harness does it
function setKeyboard(height: number) {
  Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: height > 0, __MCP_KEYBOARD_MODE__: 'overlay', __MCP_KEYBOARD_HEIGHT__: height });
  window.dispatchEvent(new Event('mcp:keyboardframe'));
}

// the harness's sheet reports how far the design system moves it up
const inset = () => screen.getByTestId('ThreadSettingsSheet').closest('[data-sheet]')?.getAttribute('data-keyboard-lift');

afterEach(() => act(async () => setKeyboard(0)));

describe('the thread settings sheet and the keyboard', () => {
  it('given the sheet is open, when the keyboard rises for its API token field, then the sheet rises above it, and it settles when the field lets go', async () => {
    // Given a thread with its settings sheet open and the keyboard down
    await app.render(`/coding-agent/${FX_THREAD_BASIC}`, { overrides: { 'coding-agent': createFixturePorts() } });
    await waitFor(() => expect(screen.getByTestId('ThreadFeed')).toBeTruthy(), { timeout: 12_000 });
    await userEvent.click(screen.getByTestId('ThreadSettingsButton'));
    const field = await screen.findByTestId('ThreadSettingsToken');
    expect(inset()).toBe('0');

    // When the person taps the token field and the keyboard rises
    await act(async () => {
      field.focus();
      setKeyboard(KEYBOARD_HEIGHT);
    });

    // Then the sheet is lifted by the keyboard's height, so Save token is not under it
    await waitFor(() => expect(inset()).toBe(String(KEYBOARD_HEIGHT)));

    // When the field lets go and the keyboard goes down
    await act(async () => {
      field.blur();
      setKeyboard(0);
    });

    // Then the sheet is back where it was
    await waitFor(() => expect(inset()).toBe('0'));
  }, 60_000);
});
