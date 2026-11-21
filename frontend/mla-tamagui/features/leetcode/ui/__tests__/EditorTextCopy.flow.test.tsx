import {
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  screen,
  waitFor,
  type FlowCodeEditor,
  type FlowSession,
} from '@lib/flow-test-harness';
import { Platform } from 'react-native';

import { app } from '../../../../tests/flow/app';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';

// The device tests read the document from CodeEditorText, a native node the app keeps in step with the page: the web view's own
// accessibility text lagged, went stale and in three hosted runs never appeared (docs/gaps/android-editor-typing-drops-a-newline.md).

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

const webPlatform = Platform.OS;
const runningOn = (os: 'android' | 'web') => Object.assign(Platform, { OS: os });
afterEach(() => {
  runningOn(webPlatform as 'web');
  useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER });
});

async function openEditor(os: 'android' | 'web'): Promise<{ session: FlowSession; editor: FlowCodeEditor }> {
  useRunnerSettingStore.setState({ runner: 'backend' });
  runningOn(os);
  const ports: LeetCodePortOverrides = {
    gateway,
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
  };
  const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
  return { session, editor: await session.editor() };
}

const copy = () => screen.getByTestId('CodeEditorText').textContent;
const eventually = (check: () => void | Promise<void>) => waitFor(check, { timeout: 8_000 });

async function raiseKeyboard(): Promise<void> {
  Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: true });
  window.dispatchEvent(new Event('mcp:keyboardframe'));
  await screen.findByTestId('EditorAction-Undo', {}, { timeout: 5_000 });
}

describe('the app copy of the editor document on Android', () => {
  it('given the editor opened on the starter code, when it hydrates, then the node holds the starter code', async () => {
    // Given / When
    const { editor } = await openEditor('android');

    // Then
    await eventually(() => expect(copy()).toBe(editor.text()));
    expect(copy()).toContain('def twoSum');
  });

  it('given the hydrated editor, when the user types and then presses Enter, then the node holds the typed text and the new line at once', async () => {
    // Given
    const { editor } = await openEditor('android');
    const starter = editor.text();

    // When
    await editor.select(starter.length);
    await editor.type('\n        seen = {}');
    await editor.press('Enter');

    // Then
    await eventually(() => expect(copy()).toBe(editor.text()));
    expect(copy().startsWith(`${starter}\n        seen = {}\n`)).toBe(true);
  });

  it('given typed text, when the toolbar Undo is tapped, then the node holds the text from before it', async () => {
    // Given
    const { session, editor } = await openEditor('android');
    const starter = editor.text();
    await editor.select(starter.length);
    await editor.type('\n        return []');
    await eventually(() => expect(copy()).toBe(editor.text()));
    await raiseKeyboard();

    // When
    await session.tap('EditorAction-Undo');

    // Then
    await eventually(() => expect(copy()).toBe(starter));
  });

  it('given a screen that is not Android, when the editor opens, then no node holds a second copy of the code', async () => {
    // Given / When
    await openEditor('web');

    // Then
    expect(screen.queryByTestId('CodeEditorText')).toBeNull();
  });
});
