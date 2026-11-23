import {
  act,
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  screen,
  userEvent,
  waitFor,
  type FlowCodeEditor,
  type FlowSession,
} from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import { useSymbolBarStore } from '../../state/editor/symbolBarStore';
import { DEFAULT_AUTOCOMPLETE, useAutocompleteSettingStore } from '../../state/autocompleteSettingStore';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';

// Suggestions on the real editor, through the screen: the list opens as a person types, a tap or the Indent key takes a suggestion, Return
// never does, and with the setting off (its default) nothing shows.

const ACCEPTED: RunResponse = { source: 'backend-run', kind: 'test', state: 'SUCCESS', requestId: 'flow-request', run_success: true, status_msg: 'Accepted', total_correct: 5, total_testcases: 5 };
const gateway: LeetCodeGatewayPort = {
  userStatus: vi.fn(async () => ({ isSignedIn: true, username: 'flow-user' })),
  test: vi.fn(async () => ({ ...ACCEPTED, interpret_id: 'flow-interpret' })),
  submit: vi.fn(async () => ACCEPTED),
  poll: vi.fn(async () => ACCEPTED),
  runLocal: vi.fn(async () => ACCEPTED),
  format: vi.fn(async (p: { lang: string; code: string }) => p.code),
  loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })),
};

// set before the screen mounts: a store write while a screen is still mounted is an update outside act
beforeEach(() => {
  useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER });
  useSymbolBarStore.setState({ railStyle: 'classic' });
  useAutocompleteSettingStore.setState({ autocomplete: DEFAULT_AUTOCOMPLETE });
});

const eventually = (check: () => void | Promise<void>, timeout = 8_000) => waitFor(check, { timeout });

async function openEditor(autocomplete: boolean): Promise<{ session: FlowSession; editor: FlowCodeEditor }> {
  useRunnerSettingStore.setState({ runner: 'backend' });
  useAutocompleteSettingStore.setState({ autocomplete });
  const ports: LeetCodePortOverrides = { gateway, voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort, audio: createFakeAudioRecorder() };
  const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
  return { session, editor: await session.editor() };
}

async function raiseKeyboard(): Promise<void> {
  Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: true });
  window.dispatchEvent(new Event('mcp:keyboardframe'));
  await waitFor(() => expect(screen.queryByTestId('EditorSyntax-Paren')).not.toBeNull(), { timeout: 5_000 });
}

// the editor holds `text` and the caret is at its end
async function typing(editor: FlowCodeEditor, text: string, keys: string) {
  await editor.setText(text);
  await editor.select(text.length);
  await editor.keys(keys);
}

describe('suggestions on the problem editor', () => {
  it('given suggestions on, when ret is typed key by key, then the list opens with return first, tapping it leaves return, and the toolbar Undo restores ret', async () => {
    const { session, editor } = await openEditor(true);
    await raiseKeyboard();

    await typing(editor, '', 'ret');
    await eventually(() => expect(editor.completions()[0]).toMatch(/^return/));
    await editor.pickCompletion('return');

    await eventually(() => expect(editor.text()).toBe('return'));
    await session.tap('EditorAction-Undo');
    await eventually(() => expect(editor.text()).toBe('ret'));
  });

  it('given the list open, when the Indent key is tapped, then return is taken, no spaces go in, and Undo restores ret', async () => {
    const { session, editor } = await openEditor(true);
    await raiseKeyboard();
    await typing(editor, '', 'ret');
    await eventually(() => expect(editor.completionOpen()).toBe(true));

    await session.tap('EditorSyntax-Tab');

    await eventually(() => expect(editor.text()).toBe('return'));
    await session.tap('EditorAction-Undo');
    await eventually(() => expect(editor.text()).toBe('ret'));
  });

  it('given the list closed, when the Indent key is tapped, then four spaces go in as before', async () => {
    const { session, editor } = await openEditor(true);
    await raiseKeyboard();
    await editor.setText('x');
    await editor.select(1);

    await session.tap('EditorSyntax-Tab');

    await eventually(() => expect(editor.text()).toBe('x    '));
  });

  it('given the list open, when Return is pressed, then a new line is made and nothing is taken', async () => {
    const { editor } = await openEditor(true);
    await typing(editor, '', 'ret');
    await eventually(() => expect(editor.completionOpen()).toBe(true));

    await editor.press('Enter');

    await eventually(() => expect(editor.text()).toBe('ret\n'));
  });

  it('given suggestions off, when ret is typed and 600 ms pass, then no list shows', async () => {
    const { editor } = await openEditor(false);

    await typing(editor, '', 'ret');
    // the editor's messages keep arriving while we wait: inside act, as every other wait of a flow test is
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 600));
    });

    expect(editor.completions()).toEqual([]);
  });

  it('given the signature line deleted, when nu is typed, then nums is still offered from the problem\'s own names', async () => {
    const { editor } = await openEditor(true);

    await typing(editor, '', 'nu');

    await eventually(() => expect(editor.completions().some((label) => label.startsWith('nums'))).toBe(true));
  });

  it('given the list open, when Done is tapped, then it closes', async () => {
    const { session, editor } = await openEditor(true);
    await raiseKeyboard();
    await typing(editor, '', 'ret');
    await eventually(() => expect(editor.completionOpen()).toBe(true));

    await session.tap('EditorAction-Done');

    await eventually(() => expect(editor.completionOpen()).toBe(false));
  });

  it('given the screen switched to JavaScript, when ret is typed, then the list offers the language\'s return', async () => {
    const { editor } = await openEditor(true);
    await userEvent.click(screen.getByTestId('ProblemEditorLanguagePicker-Trigger'));
    await waitFor(() => expect(screen.getByTestId('ProblemEditorLanguagePicker-JavaScript')).toBeTruthy());
    await userEvent.click(screen.getByTestId('ProblemEditorLanguagePicker-JavaScript'));
    await eventually(async () => expect((await editor.config()).language).toBe('javascript'));

    await typing(editor, '', 'ret');

    await eventually(() => expect(editor.completions()[0]).toMatch(/^return/));
  });
});
