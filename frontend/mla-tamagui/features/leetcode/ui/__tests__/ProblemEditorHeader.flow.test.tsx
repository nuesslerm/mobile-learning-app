import {
  act,
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  routerHarness,
  screen,
  setStackHeadersRendered,
  userEvent,
  waitFor,
  within,
  type FlowCodeEditor,
  type FlowSession,
} from '@lib/flow-test-harness';

import { app } from '../../../../tests/flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../tests/flow/loggedLines';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ProblemCatalog } from '../../domain/problemCatalog';
import { useCodeHistoryRequestStore } from '../../state/codeHistoryRequestStore';
import { useCodeHistoryStore } from '../../state/codeHistoryStore';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import { useUnsavedChangesRequestStore } from '../../state/unsavedChangesRequestStore';
import {
  useLeetCodeAuthStore,
  type LeetCodeGatewayPort,
  type LeetCodePortOverrides,
  type RunResponse,
  type VoiceEditTransportPort,
} from '../..';

// Characterisation of the editor's header as it is today (the DS ScreenHeader declared through Stack.Screen options.header):
// what each control does and where the title comes from, so the work that moves the header to a design-system spec has a net.

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

// reset before the screen mounts: a store write while a screen is still mounted is an update outside act
beforeEach(() => {
  useRunnerSettingStore.setState({ runner: 'backend' });
  useLeetCodeAuthStore.setState({ isLoggedIn: false, username: null });
  useUnsavedChangesRequestStore.getState().clear();
  useCodeHistoryRequestStore.getState().clear();
  useCodeHistoryStore.setState({ snapshots: [] });
  vi.mocked(gateway.format).mockClear();
});
afterEach(() => {
  useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER });
  useLeetCodeAuthStore.setState({ isLoggedIn: false, username: null });
});

// the editor's own start-up (frame ready, hydration, the toolbar's keyboard target) lands while the test runs, not after it
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

async function openMenu(): Promise<void> {
  await userEvent.click(screen.getByTestId('ProblemEditorMenuButton'));
  await waitFor(() => expect(screen.getByTestId('ProblemEditorMenuButtonContent')).toBeTruthy());
}

describe('the problem editor header (characterisation)', () => {
  it('given the editor route with headers rendered, when it mounts, then the header shows the catalog title and its category and difficulty', async () => {
    const problem = ProblemCatalog.getByIdOrThrow('two-sum');

    await openEditor();

    expect(screen.getByTestId('ProblemEditorHeaderTitle').textContent).toBe(problem.title);
    expect(screen.getByTestId('ProblemEditorHeaderSubtitle').textContent).toBe(
      `${problem.category} · ${problem.difficulty[0].toUpperCase()}${problem.difficulty.slice(1)}`
    );
  });

  it('given a clean buffer, when Back is pressed, then the router goes back and the screen logs editor.back as clean', async () => {
    await openEditor();
    await forgetLoggedLines();

    await userEvent.click(screen.getByTestId('ProblemEditorHeaderBack'));

    expect(routerHarness().calls()).toEqual([['back']]);
    const [line] = await loggedLines('editor.back');
    expect(line.text).toContain('editor.back');
    expect(useUnsavedChangesRequestStore.getState().payload).toBeNull();
  });

  it('given a dirty buffer, when Back is pressed, then the unsaved sheet is pushed, and discarding takes the navigate-away snapshot and goes back', async () => {
    const { editor } = await openEditor();
    await editor.select(0, editor.text().length);
    await editor.type(`${editor.text()}\n        return [0, 1]`);
    await waitFor(async () => expect(await loggedLines('editor.value_sync')).not.toHaveLength(0));

    await userEvent.click(screen.getByTestId('ProblemEditorHeaderBack'));

    expect(routerHarness().pushes()).toEqual(['/leetcode/two-sum/unsaved']);
    expect(routerHarness().calls().filter((call) => call[0] === 'back')).toEqual([]);
    const request = useUnsavedChangesRequestStore.getState().payload;
    expect(request?.problemId).toBe('two-sum');

    await act(async () => request?.onDiscard());

    expect(routerHarness().calls().at(-1)).toEqual(['back']);
    expect(useCodeHistoryStore.getState().snapshots.map((snap) => [snap.problemId, snap.reason])).toContainEqual(['two-sum', 'navigate-away']);
  });

  it('given the editor holds the caret, when the Info button is pressed, then the keyboard is dismissed and the description is pushed', async () => {
    const { editor } = await openEditor();
    await editor.focus();
    await forgetLoggedLines();

    await userEvent.click(screen.getByTestId('ProblemEditorInfoButton'));

    expect(routerHarness().pushes()).toEqual(['/leetcode/two-sum/description']);
    await waitFor(() => expect(editor.focused()).toBe(false));
    expect(await loggedLines('editor_v2.keyboard_dismiss')).not.toHaveLength(0);
  });

  it('given the More menu, when it is opened, then it lists Format, Version history and Login with their details', async () => {
    await openEditor();

    await openMenu();

    const menu = within(screen.getByTestId('ProblemEditorMenuButtonContent'));
    expect(menu.getByTestId('ProblemEditorMenuFormat').textContent).toBe('Format');
    expect(menu.getByTestId('ProblemEditorMenuHistory').textContent).toContain('No versions yet');
    expect(menu.getByTestId('ProblemEditorMenuLogin').textContent).toContain('LeetCode');
  });

  it('given the More menu, when Format is pressed, then the menu closes and the gateway formats the buffer', async () => {
    await openEditor();
    await openMenu();

    await userEvent.click(screen.getByTestId('ProblemEditorMenuFormat'));

    await waitFor(() => expect(gateway.format).toHaveBeenCalledWith(expect.objectContaining({ lang: 'python3' })));
    await waitFor(() => expect(screen.queryByTestId('ProblemEditorMenuFormat')).toBeNull());
  });

  it('given a count of saved versions, when Version history is pressed, then the restore request is registered and /history is pushed', async () => {
    useCodeHistoryStore.getState().record({ problemId: 'two-sum', language: 'python3', code: 'x = 1', reason: 'format' });
    await openEditor();
    await openMenu();
    expect(screen.getByTestId('ProblemEditorMenuHistory').textContent).toContain('1 saved');

    await userEvent.click(screen.getByTestId('ProblemEditorMenuHistory'));

    expect(routerHarness().pushes()).toEqual(['/leetcode/two-sum/history']);
    expect(useCodeHistoryRequestStore.getState().payload?.problemId).toBe('two-sum');
  });

  it('given a signed-out person, when Login is pressed, then the login route is pushed', async () => {
    await openEditor();
    await openMenu();
    expect(screen.getByTestId('ProblemEditorMenuLogin').textContent).toContain('Login');

    await userEvent.click(screen.getByTestId('ProblemEditorMenuLogin'));

    expect(routerHarness().pushes()).toEqual(['/leetcode/login']);
  });

  it('given a signed-in person, when Log out is pressed, then the credentials are cleared and nothing is pushed', async () => {
    useLeetCodeAuthStore.setState({ isLoggedIn: true, username: 'flow-user' });
    await openEditor();
    await openMenu();
    expect(screen.getByTestId('ProblemEditorMenuLogin').textContent).toContain('Log out');

    await userEvent.click(screen.getByTestId('ProblemEditorMenuLogin'));

    expect(useLeetCodeAuthStore.getState().isLoggedIn).toBe(false);
    expect(routerHarness().pushes()).toEqual([]);
  });
});
