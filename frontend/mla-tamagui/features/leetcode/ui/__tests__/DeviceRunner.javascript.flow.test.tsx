import { createFakeAudioRecorder, createFakeVoiceEditTransport, screen, userEvent, waitFor, type FlowSession } from '@lib/flow-test-harness';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../../../tests/flow/app';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';
import { createDeviceRunnerRouter } from '../../data/runner/deviceRunnerRouter';
import { createJavaScriptRunner } from '../../data/javascript/javascriptRunner';
import { createPyodideRunner } from '../../data/pyodide/pyodideRunner';
import type { RunnerLimits } from '../../data/runner/pageRunner';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import { answers, createFakePageHost, type FakePageHost, type FakePageScript } from '../../testing';

const BACKEND_RUN: RunResponse = { source: 'backend-run', kind: 'test', state: 'SUCCESS', requestId: 'backend', run_success: true, status_msg: 'Accepted', total_correct: 5, total_testcases: 5 };
const LIMITS: Partial<RunnerLimits> = { readyMs: 2000, pullMs: 2000, bootMs: 2000, waitMs: 2000, runMs: 250, rpcSlackMs: 100 };
const SEED = ['var twoSum = function(nums, target) {', '  const seen = new Map(', '};'].join('\n');
const LINE_2 = 'const seen = new Map(';

function setup(javascript: FakePageScript = {}) {
  const gateway: LeetCodeGatewayPort = {
    userStatus: vi.fn(async () => ({ isSignedIn: true, username: 'flow-user' })),
    test: vi.fn(async () => BACKEND_RUN),
    submit: vi.fn(async () => BACKEND_RUN),
    poll: vi.fn(async () => BACKEND_RUN),
    runLocal: vi.fn(async () => BACKEND_RUN),
    format: vi.fn(async (p: { lang: string; code: string }) => p.code),
    loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })),
  };
  const python: FakePageHost = createFakePageHost({ readyAfterMs: 5 });
  const js: FakePageHost = createFakePageHost({ readyAfterMs: 5, ...javascript });
  const deviceRunner = createDeviceRunnerRouter({
    python3: createPyodideRunner({ host: python, limits: LIMITS }),
    javascript: createJavaScriptRunner({ host: js, limits: LIMITS }),
  });
  return { gateway, python, js, deviceRunner };
}

async function renderEditor(gateway: LeetCodeGatewayPort, deviceRunner: LeetCodePortOverrides['deviceRunner'], search: Record<string, string> = { lang: 'javascript' }): Promise<FlowSession> {
  const ports: LeetCodePortOverrides = {
    gateway,
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
    deviceRunner,
  };
  return app.render('/leetcode/two-sum', { overrides: { leetcode: ports }, search });
}

const press = () => userEvent.click(screen.getByTestId('ConsoleBarSubmit'));
const status = () => screen.getByTestId('ConsoleResultVerdict').textContent;

afterEach(() => useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER }));

describe('Run in JavaScript on the device', () => {
  it('given the backend is chosen and the editor opens in JavaScript, when the user taps Submit, then the JavaScript page answers over 12 cases, the console says Accepted and neither the backend nor the Python page is asked', async () => {
    useRunnerSettingStore.setState({ runner: 'backend' });
    const { gateway, python, js, deviceRunner } = setup();
    await renderEditor(gateway, deviceRunner);

    await press();

    await waitFor(() => expect(status()).toBe('Accepted'));
    expect(screen.getByTestId('ConsoleResultCounts').textContent).toBe('12 / 12 testcases passed');
    expect(gateway.runLocal).not.toHaveBeenCalled();
    expect(js.loads).toBe(1);
    expect(python.loads).toBe(0);
  });

  it('given the device is chosen and the editor shows Python, when the person switches to JavaScript, then the JavaScript page is warmed and the run goes to it', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, python, js, deviceRunner } = setup();
    await renderEditor(gateway, deviceRunner, {});
    await waitFor(() => expect(python.loads).toBe(1));
    expect(js.loads).toBe(0);

    await userEvent.click(screen.getByTestId('ProblemEditorLanguagePicker-Trigger'));
    await userEvent.click(await screen.findByTestId('ProblemEditorLanguagePicker-JavaScript'));

    await waitFor(() => expect(js.loads).toBe(1));
    await press();
    await waitFor(() => expect(status()).toBe('Accepted'));
    expect(gateway.runLocal).not.toHaveBeenCalled();
  });

  it('given a compile error at line 2, when the user taps Submit, then the console shows it, offers Line 2 and the editor marks that line', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, deviceRunner } = setup({ judge: () => answers.compileError('SyntaxError: Unexpected token', { line: 2, column: 3 }) });
    const session = await renderEditor(gateway, deviceRunner, { lang: 'javascript', seed: SEED });
    const editor = await session.editor();

    await press();

    await waitFor(() => expect(screen.getByTestId('ConsoleResultErrorText').textContent).toContain('Unexpected token'));
    expect((await screen.findByTestId('ConsoleResultLine')).textContent).toContain('Line 2');
    await waitFor(async () => {
      const text = editor.text();
      expect((await editor.marks()).map((mark) => text.slice(mark.range.from, mark.range.to))).toEqual([LINE_2]);
    });
  });
});
