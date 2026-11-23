import {
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  screen,
  setStackHeadersRendered,
  userEvent,
  waitFor,
  within,
  type FlowSession,
} from '@lib/flow-test-harness';
import { describe, expect, it, vi } from 'vitest';

import { app } from '../../../../tests/flow/app';
import type {
  LeetCodeGatewayPort,
  LeetCodePortOverrides,
  RunResponse,
  VoiceEditTransportPort,
} from '../..';

// The aiEdit flag (domain/featureFlags.ts): off, the editor shows its fundamentals and none of the voice, typed-edit or plan entry points.

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

async function openEditor(flags?: { aiEdit: boolean }): Promise<FlowSession> {
  setStackHeadersRendered(true);
  const ports: LeetCodePortOverrides = {
    gateway,
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
    ...(flags ? { flags } : {}),
  };
  const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
  await session.editor();
  return session;
}

describe('the aiEdit flag', () => {
  it('given no flag is set, when the editor opens, then no voice, typed-edit or Plan control is on screen and the fundamentals are', async () => {
    await openEditor();

    expect(screen.queryByTestId('ProblemEditorVoiceStartButton')).toBeNull();
    expect(screen.queryByTestId('ProblemEditorTextEditStartButton')).toBeNull();
    expect(screen.queryByTestId('ProblemWorkspaceTab-plan')).toBeNull();
    expect(screen.queryByTestId('ProblemWorkspaceTabs')).toBeNull();
    expect(screen.getByTestId('ProblemEditorFormatButton')).toBeTruthy();
    expect(screen.getByTestId('ProblemEditorRunButton')).toBeTruthy();
    expect(screen.getByTestId('ProblemEditorResetButton')).toBeTruthy();
    await userEvent.click(screen.getByTestId('ProblemEditorMenuButton'));
    await waitFor(() => expect(screen.getByTestId('ProblemEditorMenuButtonContent')).toBeTruthy());
    expect(within(screen.getByTestId('ProblemEditorMenuButtonContent')).getByTestId('ProblemEditorMenuHistory')).toBeTruthy();
  });

  it('given aiEdit is turned on for the leetcode ports, when the editor opens, then the Plan tab, the pencil and the mic are on screen', async () => {
    await openEditor({ aiEdit: true });

    expect(screen.getByTestId('ProblemWorkspaceTab-plan')).toBeTruthy();
    expect(screen.getByTestId('ProblemEditorTextEditStartButton')).toBeTruthy();
    expect(screen.getByTestId('ProblemEditorVoiceStartButton')).toBeTruthy();
  });
});
