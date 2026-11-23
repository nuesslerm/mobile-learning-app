import {
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  screen,
  setStackHeadersRendered,
  userEvent,
  waitFor,
  type FlowCodeEditor,
  type FlowSession,
} from '@lib/flow-test-harness';

import { app } from '../../../../tests/flow/app';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { useCodeHistoryStore } from '../../state/codeHistoryStore';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';

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
  format: vi.fn(async (p: { lang: string; code: string }) => `# formatted\n${p.code}`),
  loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })),
};

beforeEach(() => {
  useRunnerSettingStore.setState({ runner: 'backend' });
  useCodeHistoryStore.setState({ snapshots: [] });
  vi.mocked(gateway.format).mockClear();
});
afterEach(() => useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER }));

async function openEditor(): Promise<{ session: FlowSession; editor: FlowCodeEditor }> {
  setStackHeadersRendered(true);
  const ports: LeetCodePortOverrides = {
    gateway,
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
  };
  const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
  return { session, editor: await session.editor() };
}

async function formatThroughTheMenu(): Promise<void> {
  await userEvent.click(screen.getByTestId('EditorSettingsButton'));
  await waitFor(() => expect(screen.getByTestId('EditorSettingsButtonContent')).toBeTruthy());
  await userEvent.click(screen.getByTestId('EditorSettingsFormat'));
  await userEvent.click(screen.getByTestId('EditorSettingsFormatConfirm'));
}

describe('Format Code through the editor settings menu', () => {
  it('given the problem editor, when the user picks Format Code and confirms, then the gateway formats and the editor buffer is replaced', async () => {
    const { editor } = await openEditor();

    await formatThroughTheMenu();

    await waitFor(() => expect(gateway.format).toHaveBeenCalled());
    await waitFor(() => expect(editor.text()).toContain('# formatted'));
  });

  it('given the code formatted through the menu, when the user taps Undo, then the code before the format returns in one step', async () => {
    const { session, editor } = await openEditor();
    const before = editor.text();

    await formatThroughTheMenu();
    await waitFor(() => expect(editor.text()).toBe(`# formatted\n${before}`), { timeout: 5_000 });
    Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: true });
    window.dispatchEvent(new Event('mcp:keyboardframe'));
    await screen.findByTestId('EditorAction-Undo', {}, { timeout: 5_000 });
    await session.tap('EditorAction-Undo');

    await waitFor(() => expect(editor.text()).toBe(before), { timeout: 5_000 });
  });
});
