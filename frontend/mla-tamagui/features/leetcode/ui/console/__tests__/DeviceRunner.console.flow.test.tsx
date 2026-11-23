import { createFakeAudioRecorder, createFakeVoiceEditTransport, expectWarning, screen, waitFor, type FlowSession } from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../../../../tests/flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../../tests/flow/loggedLines';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, VoiceEditTransportPort } from '../../..';
import { createPyodideRunner, type PyodideRunner, type RunnerLimits } from '../../../data/pyodide/pyodideRunner';
import type { JudgeParams } from '../../../data/runner/judgeProtocol';
import type { CodeRunnerPort, RunCodeParams } from '../../../domain/ports';
import { ProblemCatalog } from '../../../domain/problemCatalog';
import { useRunnerSettingStore } from '../../../state/runnerSettingStore';
import { answers, createFakePageHost, customRun, HANG, passingRun, type FakePageHost, type FakePageScript } from '../../../testing';
import { exists, fakeGateway, resetConsoleTwin, tap, text, verdict } from './consoleTwinSupport';

const LIMITS: Partial<RunnerLimits> = { readyMs: 2000, pullMs: 2000, bootMs: 2000, waitMs: 2000, runMs: 250, rpcSlackMs: 100 };

function setup(script: FakePageScript = {}) {
  const gateway = fakeGateway();
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

async function addCustomCase(): Promise<void> {
  await tap('ConsoleBarOpen');
  await waitFor(() => expect(exists('ConsoleSheet')).toBe(true));
  await tap('ConsoleChipAdd');
}

const judgedCalls = (host: FakePageHost) => host.calls.filter((call) => call.method === 'judge').map((call) => call.params as JudgeParams);

beforeEach(() => resetConsoleTwin('two-sum', 'implement-trie-prefix-tree'));

describe('Submit and Run on the device, through the console', () => {
  it('given the device is chosen, when the editor hydrates, then the device runner is warmed once with python3', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const warmed: (string | undefined)[] = [];
    const recording: CodeRunnerPort = { run: async () => passingRun(), warmUp: (lang) => void warmed.push(lang) };

    await renderEditor(setup().gateway, recording);

    await waitFor(() => expect(warmed).toEqual(['python3']));
  });

  it('given the device is chosen, when the user taps Submit, then the on-device runner answers over every case and the backend is not asked', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, host, runner } = setup();
    await renderEditor(gateway, runner);

    await tap('ConsoleBarSubmit');

    await waitFor(() => expect(verdict()).toBe('Accepted'));
    expect(text('ConsoleResultCounts')).toBe('12 / 12 testcases passed');
    expect(gateway.runLocal).not.toHaveBeenCalled();
    expect(host.loads).toBe(1);
  });

  it('given the device is chosen, when the user taps Run, then the page judges the one selected case', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, host, runner } = setup();
    await renderEditor(gateway, runner);

    await tap('ConsoleBarRun');

    await waitFor(() => expect(verdict()).toBe('Accepted'));
    expect(judgedCalls(host)[0]?.payload.testCases).toHaveLength(1);
    expect(gateway.runLocal).not.toHaveBeenCalled();
  });

  it('given the backend is chosen, when the user taps Submit, then the backend answers and no page is loaded', async () => {
    useRunnerSettingStore.setState({ runner: 'backend' });
    const { gateway, host, runner } = setup();
    await renderEditor(gateway, runner);

    await tap('ConsoleBarSubmit');

    await waitFor(() => expect(gateway.runLocal).toHaveBeenCalledTimes(1));
    expect(host.loads).toBe(0);
  });

  it('given the Trie problem route, when the user taps Submit, then the page’s judge call carries the harness and every case, and Run shows case 1 as operations and arguments', async () => {
    useRunnerSettingStore.setState({ runner: 'backend' });
    const trie = ProblemCatalog.getByIdOrThrow('implement-trie-prefix-tree');
    const cases = trie.testCases ?? [];
    const { gateway, host, runner } = setup({ judge: (call) => answers.allPass(call) });
    await renderEditor(gateway, runner, '/leetcode/implement-trie-prefix-tree');

    await tap('ConsoleBarSubmit');

    await waitFor(() => expect(verdict()).toBe('Accepted'));
    expect(text('ConsoleResultCounts')).toBe(`${cases.length} / ${cases.length} testcases passed`);
    const [call] = judgedCalls(host);
    expect(call?.payload.harness).toEqual({ kind: 'design', className: 'Trie' });
    expect(call?.payload.testCases.map((testCase) => testCase.inputs)).toEqual(cases.map((testCase) => testCase.inputs));
    expect(gateway.runLocal).not.toHaveBeenCalled();

    await tap('ConsoleFooterRun');

    await waitFor(() => expect(exists('ConsoleResultInput-operations')).toBe(true));
    expect(text('ConsoleResultInput-operations')).toContain('"Trie","insert","search"');
    expect(text('ConsoleResultInput-arguments')).toContain('["apple"]');
  });

  it('given the page answers a wrong answer, when the user taps Submit, then the console says Wrong Answer with the tally', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, runner } = setup({ judge: (call) => answers.wrongAnswer(call, 3) });
    await renderEditor(gateway, runner);

    await tap('ConsoleBarSubmit');

    await waitFor(() => expect(verdict()).toBe('Wrong Answer'));
    expect(text('ConsoleResultCounts')).toBe('11 / 12 testcases passed');
  });

  it('given the code does not compile, when the user taps Run, then the console shows the compile error', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, runner } = setup({ judge: () => answers.compileError("CompileError: expected ':'", { line: 2, column: 5 }) });
    await renderEditor(gateway, runner);

    await tap('ConsoleBarRun');

    await waitFor(() => expect(verdict()).toBe('Compile Error'));
    expect(text('ConsoleResultErrorText')).toContain("expected ':'");
  });

  it('given a case raises, when the user taps Submit, then the console shows that error as a Runtime Error', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, runner } = setup({ judge: (call) => answers.caseError(call, 'IndexError: list index out of range', { line: 4 }, 1) });
    await renderEditor(gateway, runner);

    await tap('ConsoleBarSubmit');

    await waitFor(() => expect(verdict()).toBe('Runtime Error'));
    expect(text('ConsoleResultErrorText')).toContain('IndexError: list index out of range');
  });

  it('given a page that never answers, when the limit passes, then the console reports the timeout, a fresh page is loaded, and the next run passes', async () => {
    expectWarning(/device_runner\.(timeout|page_lost)/);
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, host, runner } = setup({ judge: (call, load) => (load === 1 ? HANG : answers.allPass(call)) });
    await renderEditor(gateway, runner);

    await tap('ConsoleBarRun');
    await waitFor(() => expect(host.loads).toBe(2), { timeout: 4000 });
    await waitFor(() => expect(exists('ConsoleResultVerdict')).toBe(true), { timeout: 4000 });
    expect(text('ConsoleResultErrorText')).toContain('Timeout');
    await waitFor(() => expect(exists('ConsolePending')).toBe(false));
    await tap('ConsoleFooterRun');

    await waitFor(() => expect(verdict()).toBe('Accepted'), { timeout: 4000 });
  });

  it('given the assets cannot be loaded, when the user taps Run, then the run fails visibly and the failure is logged without the code', async () => {
    expectWarning(/device_runner\.load_failed/);
    expectWarning(/editor_v2\.run_local\.error/);
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, runner } = setup({ assetsFail: 'the runtime files are missing' });
    await renderEditor(gateway, runner);
    await forgetLoggedLines();

    await tap('ConsoleBarRun');

    await waitFor(() => expect(verdict()).toBe('Run failed'));
    expect(text('ConsoleResultErrorText')).toContain('the runtime files are missing');
    await waitFor(async () => expect((await loggedLines('device_runner.load_failed')).length).toBeGreaterThan(0));
    expect(JSON.stringify(await loggedLines('device_runner'))).not.toContain('def twoSum');
  });
});

describe('a custom case on the device, through the console', () => {
  it('given the device is chosen, when a custom case copied from Case 1 is run, then the page is asked for exactly that case with no expected value and the console shows a result with no verdict', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, host, runner } = setup({ judge: (call) => answers.custom(call, [0, 1]) });
    await renderEditor(gateway, runner);
    await addCustomCase();

    await tap('ConsoleFooterRun');

    await waitFor(() => expect(verdict()).toBe('Custom case'));
    const judged = judgedCalls(host);
    expect(judged).toHaveLength(1);
    expect(judged[0]?.payload).toMatchObject({
      testCases: [{ inputs: { nums: [2, 7, 11, 15], target: 9 }, expected: null }],
      timeoutMs: expect.any(Number),
      processLimitMs: LIMITS.runMs,
    });
    expect(text('ConsoleResultOutput')).toBe('[0,1]');
    expect(exists('ConsoleResultCounts')).toBe(false);
    expect(exists('ConsoleResultExpected')).toBe(false);
    expect(gateway.runLocal).not.toHaveBeenCalled();
  });

  it('given the page answers a raising case, when the custom case is run, then the console shows the error message', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, runner } = setup({ judge: (call) => answers.caseError(call, 'IndexError: list index out of range', { line: 4 }) });
    await renderEditor(gateway, runner);
    await addCustomCase();

    await tap('ConsoleFooterRun');

    await waitFor(() => expect(verdict()).toBe('Runtime Error'));
    expect(text('ConsoleResultErrorText')).toBe('IndexError: list index out of range');
  });

  it('given the page never answers, when the limit passes, then a fresh page is loaded and the next custom run answers', async () => {
    expectWarning(/device_runner\.(timeout|page_lost)/);
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, host, runner } = setup({ judge: (call, load) => (load === 1 ? HANG : answers.custom(call, [0, 1])) });
    await renderEditor(gateway, runner);
    await addCustomCase();

    await tap('ConsoleFooterRun');
    await waitFor(() => expect(host.loads).toBe(2), { timeout: 4000 });
    await waitFor(() => expect(exists('ConsolePending')).toBe(false), { timeout: 4000 });
    expect(text('ConsoleResultErrorText')).toContain('Timeout');
    await tap('ConsoleFooterRun');

    await waitFor(() => expect(verdict()).toBe('Custom case'), { timeout: 4000 });
  });

  it('given the backend is chosen, when a custom case is run, then the gateway receives the one case with no expected value, no page is loaded and there is no expected card', async () => {
    useRunnerSettingStore.setState({ runner: 'backend' });
    const { host, runner } = setup();
    const gateway = fakeGateway({ runLocal: vi.fn(async (params: RunCodeParams) => customRun(params)) });
    await renderEditor(gateway, runner);
    await addCustomCase();

    await tap('ConsoleFooterRun');

    await waitFor(() => expect(verdict()).toBe('Custom case'));
    expect(gateway.runLocal).toHaveBeenCalledWith(expect.objectContaining({ testCases: [{ inputs: { nums: [2, 7, 11, 15], target: 9 }, expected: null }] }));
    expect(host.loads).toBe(0);
    expect(text('ConsoleResultOutput')).toBe('[0,1]');
    expect(screen.queryByText('Expected')).toBeNull();
  });

  it('given the code does not compile, when a custom case is run, then the console shows the compile error', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { gateway, runner } = setup({ judge: () => answers.compileError("CompileError: expected ':'", { line: 2, column: 5 }) });
    await renderEditor(gateway, runner);
    await addCustomCase();

    await tap('ConsoleFooterRun');

    await waitFor(() => expect(verdict()).toBe('Compile Error'));
    expect(text('ConsoleResultErrorText')).toContain("expected ':'");
  });
});
