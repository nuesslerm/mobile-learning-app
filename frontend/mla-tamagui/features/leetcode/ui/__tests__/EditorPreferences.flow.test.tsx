import {
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  act,
  screen,
  userEvent,
  waitFor,
  type FlowCodeEditor,
  type FlowSession,
} from '@lib/flow-test-harness';

import { app } from '../../../../tests/flow/app';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { DEFAULT_EDITOR_PREFERENCES } from '../../domain/editor-settings/editorPreferences';
import { useEditorPreferencesStore } from '../../state/editor/editorPreferencesStore';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import { useToolbarConfigStore } from '../../state/toolbarConfigStore';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';

// The editor preferences reach the live page: font size, line height, wrap, Tabs and whitespace, at mount and while mounted.

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


const seed = (patch: Partial<typeof DEFAULT_EDITOR_PREFERENCES>) => useEditorPreferencesStore.setState(patch);

describe('the editor reads the editor preferences', () => {
  it('given stored preferences 16, wrap off and Tabs 2, when the editor mounts, then the page reports them with the line height', async () => {
    seed({ fontSize: 16, wrap: false, tabSize: 2 });

    const { editor } = await openEditor();

    await eventually(async () => expect(await editor.config()).toMatchObject({ fontSize: 16, lineHeight: 24, wrap: false, tabSize: 2, whitespace: false }));
  });

  it('given a mounted editor with text and a selection, when the store changes, then the page follows and the text, revision and undo are untouched', async () => {
    const { editor } = await openEditor();
    await editor.setText('x = 1');
    await editor.select(2);
    await eventually(() => expect(editor.selection()).toEqual({ start: 2, end: 2 }));
    const before = await editor.config();

    await act(async () => seed({ fontSize: 20, wrap: false, tabSize: 8 }));

    await eventually(async () => expect(await editor.config()).toMatchObject({ fontSize: 20, lineHeight: 30, wrap: false, tabSize: 8 }));
    expect(before).toMatchObject({ fontSize: 14, wrap: true, tabSize: 4 });
    expect(editor.text()).toBe('x = 1');
    expect(editor.selection()).toEqual({ start: 2, end: 2 });
    expect(await editor.undo()).toBe(true);
  });

  it('given Tabs 2, when Enter is pressed after a line that ends in a colon, then the page indents two spaces', async () => {
    seed({ tabSize: 2 });
    const { editor } = await openEditor();
    await editor.setText('if x:');
    await editor.select(5);
    await eventually(() => expect(editor.selection()).toEqual({ start: 5, end: 5 }));
    await eventually(async () => expect((await editor.config()).tabSize).toBe(2));

    await editor.press('Enter');

    await eventually(() => expect(editor.text()).toBe('if x:\n  '));
  });

  it('given preferences set, when the language is switched to JavaScript, then they still hold', async () => {
    seed({ fontSize: 18, tabSize: 2, whitespace: true });
    const { editor } = await openEditor();
    await userEvent.click(screen.getByTestId('ProblemEditorLanguagePicker-Trigger'));
    await waitFor(() => expect(screen.getByTestId('ProblemEditorLanguagePicker-JavaScript')).toBeTruthy());
    await userEvent.click(screen.getByTestId('ProblemEditorLanguagePicker-JavaScript'));

    await eventually(async () => expect(await editor.config()).toMatchObject({ language: 'javascript', fontSize: 18, lineHeight: 27, tabSize: 2, whitespace: true }));
  });
});

describe('Show Whitespace on the real page', () => {
  it('given whitespace on, when the editor is ready, then the page reports it and draws whitespace marks', async () => {
    seed({ whitespace: true });
    const { editor } = await openEditor();
    await editor.setText('a  b\n    c');

    await eventually(async () => expect((await editor.config()).whitespace).toBe(true));
    await eventually(() => expect(editor.whitespaceMarks()).toBeGreaterThan(0));
  });

  it('given whitespace on and then off while mounted, when it turns off, then no marks remain and text, selection and undo are unchanged', async () => {
    seed({ whitespace: true });
    const { editor } = await openEditor();
    await editor.setText('a  b');
    await editor.select(2);
    await eventually(() => expect(editor.whitespaceMarks()).toBeGreaterThan(0));

    await act(async () => seed({ whitespace: false }));

    await eventually(() => expect(editor.whitespaceMarks()).toBe(0));
    expect(editor.text()).toBe('a  b');
    expect(editor.selection()).toEqual({ start: 2, end: 2 });
    expect(await editor.undo()).toBe(true);
  });
});
