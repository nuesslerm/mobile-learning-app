import {
  act,
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  routerHarness,
  screen,
  setLiquidGlass,
  setStackHeadersRendered,
  userEvent,
  waitFor,
  within,
  type FlowSession,
} from '@lib/flow-test-harness';

import { app } from '../../../../tests/flow/app';
import { useNativeHeaderStore } from '@lib/tamagui-design-system';
import { Platform } from 'react-native';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ProblemCatalog } from '../../domain/problemCatalog';
import { useCodeHistoryStore } from '../../state/codeHistoryStore';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import { useUnsavedChangesRequestStore } from '../../state/unsavedChangesRequestStore';
import { useLeetCodeAuthStore, type CodeRunnerPort, type LeetCodeGatewayPort, type LeetCodePortOverrides, type RunResponse, type VoiceEditTransportPort } from '../..';

// The editor's glass layout through the native twin (iOS with Liquid Glass, the setting on): wiring, grouping and the busy piece. The glass
// itself, the morph and UIKit's menu behaviour are the phone's to show.

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

const realOS = Platform.OS;

beforeEach(() => {
  useRunnerSettingStore.setState({ runner: 'backend' });
  useLeetCodeAuthStore.setState({ isLoggedIn: false, username: null });
  useUnsavedChangesRequestStore.getState().clear();
  useCodeHistoryStore.setState({ snapshots: [] });
  useNativeHeaderStore.setState({ enabled: true });
});
afterEach(() => {
  (Platform as { OS: string }).OS = realOS;
  useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER });
});

type Deferred = { runner: CodeRunnerPort; finish: () => void };

function pendingRunner(): Deferred {
  let release: (response: RunResponse) => void = () => {};
  const waiting = new Promise<RunResponse>((resolve) => {
    release = resolve;
  });
  return { runner: { run: () => waiting }, finish: () => release(ACCEPTED) };
}

// The route's native modules load on the web platform, so iOS comes on once the screen is up and a store write makes the bar re-render into the native one.
async function openEditor(platform: 'ios' | 'web', runner?: CodeRunnerPort): Promise<FlowSession> {
  setStackHeadersRendered(true);
  if (platform === 'ios') useNativeHeaderStore.setState({ enabled: false });
  const ports: LeetCodePortOverrides = {
    gateway,
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
    ...(runner ? { runner } : {}),
  };
  const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
  if (platform === 'ios') {
    (Platform as { OS: string }).OS = 'ios';
    setLiquidGlass(true);
    await act(async () => useNativeHeaderStore.setState({ enabled: true }));
  }
  return session;
}

describe('the problem editor header on the native twin', () => {
  it('given iOS with Liquid Glass and the setting on, when the editor opens, then the bar reads back the round back, the capsule, the settings piece alone and Info with More grouped', async () => {
    const problem = ProblemCatalog.getByIdOrThrow('two-sum');

    await openEditor('ios');

    expect(screen.getByTestId('NativeHeaderItem-Back to the problem list').getAttribute('data-shares-background')).toBe('false');
    const capsule = screen.getByTestId('ScreenHeaderGlassTitle');
    expect(within(capsule).getByText(problem.title)).toBeTruthy();
    expect(within(capsule).getByText(`${problem.category} · ${problem.difficulty[0]?.toUpperCase()}${problem.difficulty.slice(1)}`)).toBeTruthy();
    expect(screen.getByTestId('NativeHeaderItem-Editor settings').getAttribute('data-shares-background')).toBe('false');
    expect(screen.getByTestId('NativeHeaderItem-Editor settings').textContent).toBe('Python');
    expect(screen.getByTestId('NativeHeaderItem-Problem description').getAttribute('data-shares-background')).toBe('true');
    expect(screen.getByTestId('NativeHeaderItem-More actions').getAttribute('data-shares-background')).toBe('true');
    expect(screen.queryByTestId('ProblemEditorHeader')).toBeNull();
    expect(screen.queryByTestId('NativeHeaderCustom')).toBeNull();
  });

  it('given the native bar, when the settings pill is opened, then the menu has the two sections and More has no Format row', async () => {
    await openEditor('ios');

    await userEvent.click(screen.getByTestId('NativeHeaderItem-Editor settings'));
    const menu = await waitFor(() => screen.getByTestId('NativeHeaderMenu-Editor settings'));

    expect(within(menu).getByTestId('NativeHeaderMenuItem-Font size: 14px')).toBeTruthy();
    expect(within(menu).getByTestId('NativeHeaderMenuItem-Line wrap').getAttribute('data-keeps-menu-presented')).toBe('true');
    expect(within(menu).getByTestId('NativeHeaderMenuItem-Symbol Bar').getAttribute('data-keeps-menu-presented')).toBe('false');
    await userEvent.click(screen.getByTestId('NativeHeaderItem-Editor settings'));
    await userEvent.click(screen.getByTestId('NativeHeaderItem-More actions'));
    const more = await waitFor(() => screen.getByTestId('NativeHeaderMenu-More actions'));
    expect(within(more).queryByTestId('NativeHeaderMenuItem-Format')).toBeNull();
    expect(within(more).getByTestId('NativeHeaderMenuItem-Version history')).toBeTruthy();
  });

  it('given a run in flight, when it finishes, then the busy piece shows only while it runs', async () => {
    const run = pendingRunner();
    await openEditor('ios', run.runner);
    expect(screen.queryByTestId('NativeHeaderCustom')).toBeNull();

    await userEvent.click(screen.getByTestId('ConsoleBarSubmit'));

    await waitFor(() => expect(within(screen.getByTestId('NativeHeaderCustom')).getByLabelText('Working')).toBeTruthy());
    await act(async () => run.finish());
    await waitFor(() => expect(screen.queryByTestId('NativeHeaderCustom')).toBeNull());
  });

  it('given a clean buffer, when Back is pressed, then the router goes back; and Info pushes the description', async () => {
    await openEditor('ios');

    await userEvent.click(screen.getByTestId('NativeHeaderItem-Problem description'));
    await userEvent.click(screen.getByTestId('NativeHeaderItem-Back to the problem list'));

    expect(routerHarness().pushes()).toEqual(['/leetcode/two-sum/description']);
    expect(routerHarness().calls().at(-1)).toEqual(['back']);
  });

  it('given the setting on under the classic bar (web), when a run is in flight, then the spinner and the two pills render with the same ids', async () => {
    const run = pendingRunner();
    await openEditor('web', run.runner);

    expect(screen.getByTestId('ProblemEditorHeaderGroup0')).toBeTruthy();
    expect(screen.getByTestId('ProblemEditorHeaderGroup1')).toBeTruthy();
    expect(within(screen.getByTestId('ProblemEditorHeaderGroup1')).getByTestId('ProblemEditorInfoButton')).toBeTruthy();
    expect(within(screen.getByTestId('ProblemEditorHeaderGroup1')).getByTestId('ProblemEditorMenuButton')).toBeTruthy();
    expect(screen.queryByLabelText('Working')).toBeNull();

    await userEvent.click(screen.getByTestId('ConsoleBarSubmit'));

    await waitFor(() => expect(screen.getByLabelText('Working')).toBeTruthy());
    await act(async () => run.finish());
    await waitFor(() => expect(screen.queryByLabelText('Working')).toBeNull());
  });
});
