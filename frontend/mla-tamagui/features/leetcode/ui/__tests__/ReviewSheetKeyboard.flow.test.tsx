import { act, createFakeAudioRecorder, createFakeVoiceEditTransport, screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../../../tests/flow/app';
import type { LeetCodeGatewayPort, VoiceEditTransportPort } from '../..';

const KEYBOARD_HEIGHT = 300;

function gateway(): LeetCodeGatewayPort {
  return {
    userStatus: vi.fn(async () => ({ isSignedIn: true, username: 'flow-user' })),
    test: vi.fn(),
    submit: vi.fn(),
    poll: vi.fn(),
    runLocal: vi.fn(),
    format: vi.fn(),
    loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })),
  };
}

// the keyboard up, signalled the way the device harness does it
function setKeyboard(height: number) {
  Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: height > 0, __MCP_KEYBOARD_MODE__: 'overlay', __MCP_KEYBOARD_HEIGHT__: height });
  window.dispatchEvent(new Event('mcp:keyboardframe'));
}

// the harness's sheet reports the inset the design system asks of it
const inset = () => screen.getByTestId('ProblemEditorVoiceReviewSheet').closest('[data-sheet]')?.getAttribute('data-bottom-inset');

afterEach(() => act(async () => setKeyboard(0)));

describe('the voice-edit review sheet and the keyboard', () => {
  it('given the sheet is open in text mode, when the keyboard rises for its transcript field, then the sheet rises above it, and it settles when the field lets go', async () => {
    // Given the editor with its text-edit sheet open and the keyboard down
    await app.render('/leetcode/two-sum', {
      overrides: { leetcode: { gateway: gateway(), voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort, audio: createFakeAudioRecorder() } },
    });
    await userEvent.click(screen.getByTestId('ProblemEditorTextEditStartButton'));
    const field = await screen.findByTestId('ProblemEditorVoiceReviewSheet-TranscriptInput');
    expect(inset()).toBe('0');

    // When the person taps the field and the keyboard rises
    await act(async () => {
      field.focus();
      setKeyboard(KEYBOARD_HEIGHT);
    });

    // Then the sheet is lifted by the keyboard's height, so Send is not under it
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
