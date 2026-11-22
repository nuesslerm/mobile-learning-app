import { createFakeAudioRecorder, createFakeVoiceEditTransport, expectWarning, screen, userEvent, waitFor, type FlowSession } from '@lib/flow-test-harness';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../tests/flow/loggedLines';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';
import { createPyodideRunner, type PyodideRunner, type RunnerLimits } from '../../data/pyodide/pyodideRunner';
import type { CodeRunnerPort, RunCodeParams } from '../../domain/ports';
import { ProblemCatalog } from '../../domain/problemCatalog';
import type { JudgeParams } from '../../data/runner/judgeProtocol';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import { answers, createFakePageHost, customRun, HANG, passingRun, type FakePageHost, type FakePageScript } from '../../testing';

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

async function renderEditor(gateway: LeetCodeGatewayPort, runner: CodeRunnerPort, route = '/leetcode/two-sum'): Promise<FlowSession> {
  const ports: LeetCodePortOverrides = {
    gateway,
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
    deviceRunner: runner,
  };
  const session = await app.render(route, { overrides: { leetcode: ports } });
  await session.editor();
  return session;
}

const press = () => userEvent.click(screen.getByTestId('ProblemEditorRunLocalButton'));
const status = () => screen.getByTestId('ProblemEditorRunOutcomeStatus').textContent;

afterEach(() => useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER }));

describe('Run Local on the device', () => {
  it('given the device is chosen, when the editor hydrates, then the device runner is warmed once with python3', async () => {
    // Given the device chosen and a device runner that only records its warm-ups
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway } = setup();
    const warmed: (string | undefined)[] = [];
    const recording: CodeRunnerPort = { run: async () => passingRun(), warmUp: (lang) => void warmed.push(lang) };

    // When the editor opens two-sum and hydrates
    await renderEditor(gateway, recording);

    // Then it was warmed once, for Python
    await waitFor(() => expect(warmed).toEqual(['python3']));
  });

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

  it('given the backend is chosen and the Trie problem route, when the user runs the tests, then the page’s judge call carries the harness and the operations and arguments, the card shows case 1 as operations and arguments, and the backend is not asked', async () => {
    // Given the backend chosen and a real catalog problem the backend cannot run (a design problem with a harness)
    useRunnerSettingStore.setState({ runner: 'backend' });
    const trie = ProblemCatalog.getByIdOrThrow('implement-trie-prefix-tree');
    const cases = trie.testCases ?? [];
    const calls: JudgeParams[] = [];
    const { gateway, runner } = setup({
      judge: (call) => {
        calls.push(call);
        return answers.allPass(call);
      },
    });
    await renderEditor(gateway, runner, '/leetcode/implement-trie-prefix-tree');

    // When the user presses Run Local
    await press();

    // Then the page answers over every bundled case, its call carries the harness and each case's operations and arguments, and the backend was never asked
    await waitFor(() => expect(status()).toBe('Accepted'));
    expect(screen.getByTestId('ProblemEditorRunOutcomeTotals').textContent).toBe(`${cases.length} / ${cases.length} test cases passed`);
    expect(calls[0]?.payload.harness).toEqual({ kind: 'design', className: 'Trie' });
    expect(calls[0]?.payload.testCases.map((testCase) => testCase.inputs)).toEqual(cases.map((testCase) => testCase.inputs));
    expect(gateway.runLocal).not.toHaveBeenCalled();

    // And case 1 reads as its operations then its arguments, clipped to a line until a tap opens it
    expect(screen.getByTestId('ProblemEditorRunCase-0-Input').textContent).toMatch(/^operations = \["Trie","insert","search".*…$/);
    await userEvent.click(screen.getByTestId('ProblemEditorRunCase-0-Input-Toggle'));
    expect(screen.getByTestId('ProblemEditorRunCase-0-Input').textContent).toContain('], arguments = [[],["apple"]');
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
    expectWarning(/device_runner\.(timeout|page_lost)/);
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
    expectWarning(/device_runner\.load_failed/);
    expectWarning(/editor_v2\.run_local\.error/);
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, runner } = setup({ assetsFail: 'the runtime files are missing' });
    await renderEditor(gateway, runner);
    await forgetLoggedLines();
    await press();
    await waitFor(() => expect(screen.getByTestId('ProblemEditorRunOutcomeRuntimeError').textContent).toContain('the runtime files are missing'));
    expect(status()).toBe('Error');
    await waitFor(async () => expect((await loggedLines('device_runner.load_failed')).length).toBeGreaterThan(0));
    expect(JSON.stringify(await loggedLines('device_runner'))).not.toContain('def twoSum');
  });
});

const pressInput = () => userEvent.click(screen.getByTestId('ProblemEditorRunInputButton'));
const customError = () => screen.getByTestId('ProblemEditorCustomCase-Error').textContent;

describe('Run input on the device', () => {
  it('given the device is chosen, when the learner presses Run input on the default input, then the page is asked for exactly that one case with no expected value, the backend is not asked and the card shows the page\'s result', async () => {
    // Given the device chosen and a page that answers a typed case as the real judge does (pass false, expected null)
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, host, runner } = setup({ judge: (call) => answers.custom(call, [0, 1]) });
    await renderEditor(gateway, runner);

    // When the learner presses Run input
    await pressInput();

    // Then the page's judge call carried the one case, with the run's limits, and the card shows its result with no verdict
    await waitFor(() => expect(status()).toBe('Custom case'));
    const judged = host.calls.filter((call) => call.method === 'judge').map((call) => call.params as JudgeParams);
    expect(judged).toHaveLength(1);
    expect(judged[0]?.payload).toMatchObject({
      testCases: [{ inputs: { nums: [2, 7, 11, 15], target: 9 }, expected: null }],
      timeoutMs: expect.any(Number),
      processLimitMs: LIMITS.runMs,
    });
    expect(screen.getByTestId('ProblemEditorCustomCase-Result').textContent).toBe('[0,1]');
    expect(screen.queryByTestId('ProblemEditorRunOutcomeTotals')).toBeNull();
    expect(gateway.runLocal).not.toHaveBeenCalled();
  });

  it('given the device is chosen and the page answers a raising case, when Run input is pressed, then the card shows the Error row with that message', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, runner } = setup({ judge: (call) => answers.caseError(call, 'IndexError: list index out of range', { line: 4 }) });
    await renderEditor(gateway, runner);

    await pressInput();

    await waitFor(() => expect(status()).toBe('Error'));
    expect(customError()).toBe('IndexError: list index out of range');
    expect(screen.getByTestId('ProblemEditorCustomCase-Result').textContent).toBe('—');
  });

  it('given the page never answers, when the limit passes, then the custom card says Timeout, a fresh page is loaded, and the next Run input answers', async () => {
    expectWarning(/device_runner\.(timeout|page_lost)/);
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, host, runner } = setup({ judge: (call, load) => (load === 1 ? HANG : answers.custom(call, [0, 1])) });
    await renderEditor(gateway, runner);

    await pressInput();
    await waitFor(() => expect(customError()).toContain('Timeout'), { timeout: 4000 });
    await waitFor(() => expect(host.loads).toBe(2));
    await pressInput();

    await waitFor(() => expect(status()).toBe('Custom case'), { timeout: 4000 });
  });

  it('given the backend is chosen, when Run input is pressed, then the gateway receives the one case with no expected value, no page is loaded and the card shows the backend\'s row without a verdict', async () => {
    useRunnerSettingStore.setState({ runner: 'backend' });
    const { gateway, host, runner } = setup();
    gateway.runLocal = vi.fn(async (params: RunCodeParams) => customRun(params));
    await renderEditor(gateway, runner);

    await pressInput();

    await waitFor(() => expect(status()).toBe('Custom case'));
    expect(gateway.runLocal).toHaveBeenCalledWith(expect.objectContaining({ testCases: [{ inputs: { nums: [2, 7, 11, 15], target: 9 }, expected: null }] }));
    expect(host.loads).toBe(0);
    expect(screen.getByTestId('ProblemEditorCustomCase-Result').textContent).toBe('[0,1]');
    expect(screen.queryByText('Expected')).toBeNull();
  });

  it('given the device is chosen and the code does not compile, when Run input is pressed, then the card shows the compile error and still echoes the typed input', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, runner } = setup({ judge: () => answers.compileError("CompileError: expected ':'", { line: 2, column: 5 }) });
    await renderEditor(gateway, runner);

    await pressInput();

    await waitFor(() => expect(status()).toBe('Error'));
    expect(customError()).toContain("expected ':'");
    expect(screen.getByTestId('ProblemEditorCustomCase-Input').textContent).toBe('nums = [2,7,11,15], target = 9');
  });
});
