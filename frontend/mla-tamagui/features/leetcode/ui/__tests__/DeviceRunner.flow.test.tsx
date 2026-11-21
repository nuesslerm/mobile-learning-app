import { createFakeAudioRecorder, createFakeVoiceEditTransport, expectWarning, screen, userEvent, waitFor, type FlowSession } from '@lib/flow-test-harness';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../tests/flow/loggedLines';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';
import { createPyodideRunner, type PyodideRunner, type RunnerLimits } from '../../data/pyodide/pyodideRunner';
import { ProblemCatalog } from '../../domain/problemCatalog';
import type { JudgeParams } from '../../data/pyodide/judgeProtocol';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import { answers, createFakePageHost, HANG, type FakePageHost, type FakePageScript } from '../../testing';

const BACKEND_RUN: RunResponse = { source: 'backend-run', kind: 'test', state: 'SUCCESS', requestId: 'backend', run_success: true, status_msg: 'Accepted', total_correct: 5, total_testcases: 5 };
const LIMITS: Partial<RunnerLimits> = { readyMs: 2000, pullMs: 2000, bootMs: 2000, waitMs: 2000, runMs: 250, rpcSlackMs: 100 };

function setup(script: FakePageScript = {}) {
  const gateway: LeetCodeGatewayPort = {
    userStatus: vi.fn(async () => ({ isSignedIn: true, username: 'flow-user' })),
    test: vi.fn(async () => BACKEND_RUN),
    submit: vi.fn(async () => BACKEND_RUN),
    poll: vi.fn(async () => BACKEND_RUN),
    runLocal: vi.fn(async () => BACKEND_RUN),
    format: vi.fn(async (p: { lang: string; code: string }) => p.code),
    loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })),
  };
  const host: FakePageHost = createFakePageHost({ readyAfterMs: 5, ...script });
  const runner: PyodideRunner = createPyodideRunner({ host, limits: LIMITS });
  return { gateway, host, runner };
}

async function renderEditor(gateway: LeetCodeGatewayPort, runner: PyodideRunner): Promise<FlowSession> {
  const ports: LeetCodePortOverrides = {
    gateway,
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
    deviceRunner: runner,
  };
  const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
  await session.editor();
  return session;
}

const press = () => userEvent.click(screen.getByTestId('ProblemEditorRunLocalButton'));
const status = () => screen.getByTestId('ProblemEditorRunOutcomeStatus').textContent;

afterEach(() => useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER }));

describe('Run Local on the device', () => {
  it('given the device is chosen, when the user runs the tests, then the on-device runner answers over every case and the backend is not asked', async () => {
    // Given the editor with the device chosen, and a page that has to be loaded by the first run
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, host, runner } = setup();
    await renderEditor(gateway, runner);

    // When the user presses Run Local
    await press();

    // Then the card shows the device's verdict over the catalog's 12 cases, the page was loaded once, the backend never asked
    await waitFor(() => expect(status()).toBe('Accepted'));
    expect(screen.getByTestId('ProblemEditorRunOutcomeTotals').textContent).toBe('12 / 12 test cases passed');
    expect(gateway.runLocal).not.toHaveBeenCalled();
    expect(host.loads).toBe(1);
  });

  it('given the backend is chosen, when the user runs the tests, then the backend answers and no page is loaded', async () => {
    useRunnerSettingStore.setState({ runner: 'backend' });
    const { gateway, host, runner } = setup();
    await renderEditor(gateway, runner);
    await press();
    await waitFor(() => expect(gateway.runLocal).toHaveBeenCalledTimes(1));
    expect(host.loads).toBe(0);
  });

  it('given the backend is chosen and a problem with a harness, when the user runs the tests, then the device runner answers with the harness in its call and the backend is not asked', async () => {
    // Given the backend chosen and a problem the backend cannot run (no catalog problem has a harness yet, so two-sum is given one for this test)
    useRunnerSettingStore.setState({ runner: 'backend' });
    const problem = ProblemCatalog.getByIdOrThrow('two-sum');
    problem.harness = { kind: 'design', className: 'Trie' };
    try {
      const calls: JudgeParams[] = [];
      const { gateway, runner } = setup({
        judge: (call) => {
          calls.push(call);
          return answers.allPass(call);
        },
      });
      await renderEditor(gateway, runner);

      // When the user presses Run Local
      await press();

      // Then the page answers, its call carries the harness, and the backend was never asked
      await waitFor(() => expect(status()).toBe('Accepted'));
      expect(calls[0]?.payload.harness).toEqual({ kind: 'design', className: 'Trie' });
      expect(gateway.runLocal).not.toHaveBeenCalled();
    } finally {
      delete problem.harness;
    }
  });

  it('given the page answers a wrong answer, when the user runs the tests, then the card says Wrong Answer with the tally', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, runner } = setup({ judge: (call) => answers.wrongAnswer(call, 3) });
    await renderEditor(gateway, runner);
    await press();
    await waitFor(() => expect(status()).toBe('Wrong Answer'));
    expect(screen.getByTestId('ProblemEditorRunOutcomeTotals').textContent).toBe('11 / 12 test cases passed');
  });

  it('given the code does not compile, when the user runs the tests, then the card shows the compile error', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, runner } = setup({ judge: () => answers.compileError("CompileError: expected ':'", { line: 2, column: 5 }) });
    await renderEditor(gateway, runner);
    await press();
    await waitFor(() => expect(screen.getByTestId('ProblemEditorRunOutcomeCompileError').textContent).toContain("expected ':'"));
  });

  it('given a case raises, when the user runs the tests, then the card shows that error beside Wrong Answer', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, runner } = setup({ judge: (call) => answers.caseError(call, 'IndexError: list index out of range', { line: 4 }, 1) });
    await renderEditor(gateway, runner);
    await press();
    await waitFor(() => expect(screen.getByTestId('ProblemEditorRunOutcomeRuntimeError').textContent).toContain('IndexError: list index out of range'));
    expect(status()).toBe('Wrong Answer');
  });

  it('given a page that never answers, when the limit passes, then the card says Timeout, a fresh page is loaded, and the next run passes', async () => {
    expectWarning(/pyodide_runner\.(timeout|page_lost)/);
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, host, runner } = setup({ judge: (call, load) => (load === 1 ? HANG : answers.allPass(call)) });
    await renderEditor(gateway, runner);
    await press();
    await waitFor(() => expect(screen.getByTestId('ProblemEditorRunOutcomeRuntimeError').textContent).toContain('Timeout'), { timeout: 4000 });
    await waitFor(() => expect(host.loads).toBe(2));
    await press();
    await waitFor(() => expect(status()).toBe('Accepted'), { timeout: 4000 });
  });

  it('given the assets cannot be loaded, when the user runs the tests, then the run fails visibly and the failure is logged without the code', async () => {
    expectWarning(/pyodide_runner\.load_failed/);
    expectWarning(/editor_v2\.run_local\.error/);
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, runner } = setup({ assetsFail: 'the runtime files are missing' });
    await renderEditor(gateway, runner);
    await forgetLoggedLines();
    await press();
    await waitFor(() => expect(screen.getByTestId('ProblemEditorRunOutcomeRuntimeError').textContent).toContain('the runtime files are missing'));
    expect(status()).toBe('Error');
    await waitFor(async () => expect((await loggedLines('pyodide_runner.load_failed')).length).toBeGreaterThan(0));
    expect(JSON.stringify(await loggedLines('pyodide_runner'))).not.toContain('def twoSum');
  });
});
