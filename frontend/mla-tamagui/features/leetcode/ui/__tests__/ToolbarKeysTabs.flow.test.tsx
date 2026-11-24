import {
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  screen,
  waitFor,
  type FlowCodeEditor,
  type FlowSession,
} from '@lib/flow-test-harness';

import { app } from '../../../../tests/flow/app';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { DEFAULT_EDITOR_PREFERENCES } from '../../domain/editor-settings/editorPreferences';
import { useEditorPreferencesStore } from '../../state/editor/editorPreferencesStore';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import { useSymbolBarStore } from '../../state/editor/symbolBarStore';
import { useToolbarConfigStore } from '../../state/toolbarConfigStore';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';

// The Tabs preference is the unit of the toolbar's own edits: Tab, Dedent and Newline. The editor page's own indentation follows
// the same preference once the editor reads it (a later slice), so these cases tap the toolbar and read the text it leaves.

const ACCEPTED: RunResponse = {
  source: 'backend-run',
  kind: 'test',
  state: 'SUCCESS',
  requestId: 'flow-request',
  run_success: true,
  status_msg: 'Accepted',
  total_correct: 5,
  total_testcases: 5,
};

const gateway: LeetCodeGatewayPort = {
  userStatus: vi.fn(async () => ({ isSignedIn: true, username: 'flow-user' })),
  test: vi.fn(async () => ({ ...ACCEPTED, interpret_id: 'flow-interpret' })),
  submit: vi.fn(async () => ACCEPTED),
  poll: vi.fn(async () => ACCEPTED),
  runLocal: vi.fn(async () => ACCEPTED),
  format: vi.fn(async (p: { lang: string; code: string }) => p.code),
  loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })),
};

// reset before the screen mounts: a store write while a screen is still mounted is an update outside act
beforeEach(() => {
  useRunnerSettingStore.setState({ runner: 'backend' });
  useSymbolBarStore.setState({ railStyle: 'classic' });
  useToolbarConfigStore.getState().resetToDefault();
  useEditorPreferencesStore.setState(DEFAULT_EDITOR_PREFERENCES);
});
afterEach(() => useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER }));

// each key crosses the WebView twin as one or two messages, about a second on a loaded machine
const eventually = (check: () => void | Promise<void>, timeout = 8_000) => waitFor(check, { timeout });

async function openEditor(): Promise<{ session: FlowSession; editor: FlowCodeEditor }> {
  const ports: LeetCodePortOverrides = {
    gateway,
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
  };
  const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
  return { session, editor: await session.editor() };
}

// The soft keyboard up, signalled the way the device harness does: that raises the toolbar.
async function raiseKeyboard(): Promise<void> {
  Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: true });
  window.dispatchEvent(new Event('mcp:keyboardframe'));
  await waitFor(() => expect(screen.queryByTestId('EditorSyntax-Paren')).not.toBeNull(), { timeout: 5_000 });
}

// The editor holds `code` with the selection set, the keyboard is up, and the screen has seen both.
async function prepare(code: string, start: number, end = start) {
  const opened = await openEditor();
  await opened.editor.setText(code);
  await opened.editor.select(start, end);
  await raiseKeyboard();
  await eventually(() => expect(opened.editor.text()).toBe(code));
  await eventually(() => expect(opened.editor.selection()).toEqual({ start, end }));
  return opened;
}

const caret = (at: number) => ({ start: at, end: at });

describe('the syntax toolbar with a Tabs preference', () => {
  it('given Tabs 2, when Tab is tapped at the caret, then two spaces go in and the caret follows them', async () => {
    useEditorPreferencesStore.setState({ tabSize: 2 });
    const { session, editor } = await prepare('x = 1', 0);

    await session.tap('EditorSyntax-Tab');

    await eventually(() => expect(editor.text()).toBe('  x = 1'));
    await eventually(() => expect(editor.selection()).toEqual(caret(2)));
  });

  it('given Tabs 2 and two lines selected, when Tab is tapped, then both lines gain two spaces and the block stays selected', async () => {
    useEditorPreferencesStore.setState({ tabSize: 2 });
    const { session, editor } = await prepare('a\nb', 0, 3);

    await session.tap('EditorSyntax-Tab');

    await eventually(() => expect(editor.text()).toBe('  a\n  b'));
    await eventually(() => expect(editor.selection()).toEqual({ start: 0, end: 7 }));
  });

  it('given Tabs 2 and a line indented four spaces, when Dedent is tapped, then two spaces go', async () => {
    useEditorPreferencesStore.setState({ tabSize: 2 });
    const { session, editor } = await prepare('    x', 5);

    await session.tap('EditorSyntax-Dedent');

    await eventually(() => expect(editor.text()).toBe('  x'));
    await eventually(() => expect(editor.selection()).toEqual(caret(3)));
  });

  it('given Tabs 2 and a line that ends in a colon, when Newline is tapped, then the next line starts two spaces deeper', async () => {
    useEditorPreferencesStore.setState({ tabSize: 2 });
    const { session, editor } = await prepare('if x:', 5);

    await session.tap('EditorSyntax-Newline');

    await eventually(() => expect(editor.text()).toBe('if x:\n  '));
    await eventually(() => expect(editor.selection()).toEqual(caret(8)));
  });

  it('given Tabs 8, when Tab, Dedent and Newline are tapped, then each moves eight spaces', async () => {
    useEditorPreferencesStore.setState({ tabSize: 8 });
    const { session, editor } = await prepare('x', 0);

    await session.tap('EditorSyntax-Tab');
    await eventually(() => expect(editor.text()).toBe('        x'));
    await session.tap('EditorSyntax-Dedent');
    await eventually(() => expect(editor.text()).toBe('x'));
    await editor.setText('if x:');
    await editor.select(5, 5);
    await eventually(() => expect(editor.selection()).toEqual(caret(5)));
    await session.tap('EditorSyntax-Newline');

    await eventually(() => expect(editor.text()).toBe('if x:\n        '));
  });
});
