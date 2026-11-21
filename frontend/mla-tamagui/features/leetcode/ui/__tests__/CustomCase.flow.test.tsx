import { createFakeAudioRecorder, createFakeVoiceEditTransport, expectWarning, fireEvent, screen, userEvent, waitFor, type FlowSession } from '@lib/flow-test-harness';
import { describe, expect, it, vi } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../tests/flow/loggedLines';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';
import { CodeRunnerError } from '../../domain/ports';
import { ProblemCatalog } from '../../domain/problemCatalog';
import { createFakeCodeRunner, customRun, passingRun, type FakeCodeRunner } from '../../testing';

function gateway(): LeetCodeGatewayPort {
  return {
    userStatus: vi.fn(async () => ({ isSignedIn: true, username: 'flow-user' })),
    test: vi.fn(),
    submit: vi.fn(),
    poll: vi.fn(),
    runLocal: vi.fn(),
    format: vi.fn(async (p: { lang: string; code: string }) => p.code),
    loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })),
  } as unknown as LeetCodeGatewayPort;
}

async function renderEditor(runner: FakeCodeRunner, route = '/leetcode/two-sum'): Promise<FlowSession> {
  const ports: LeetCodePortOverrides = {
    gateway: gateway(),
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
    runner,
  };
  const session = await app.render(route, { overrides: { leetcode: ports } });
  await session.editor();
  return session;
}

// fireEvent, not userEvent.type: that reads '[' and '{' as key descriptors
const typeInput = (text: string) => fireEvent.change(screen.getByTestId('ProblemEditorTestInput'), { target: { value: text } });
const pressRunInput = () => userEvent.click(screen.getByTestId('ProblemEditorRunInputButton'));
const pressRunLocal = () => userEvent.click(screen.getByTestId('ProblemEditorRunLocalButton'));
// userEvent refuses a button whose pointer events are off, so a tap that still arrives is a bare click
const strayClick = (id: string) => fireEvent.click(screen.getByTestId(id));
const text = (id: string) => screen.getByTestId(id).textContent;
const isDisabled = (id: string) => (screen.getByTestId(id) as HTMLButtonElement).disabled || screen.getByTestId(id).getAttribute('aria-disabled') === 'true';

const customRunner = () => createFakeCodeRunner((params) => customRun(params));

describe('Run input', () => {
  it('given two-sum open with the default input, when the learner presses Run input, then the runner is asked for that one case with no expected value and the card shows what came back and no verdict', async () => {
    // Given the editor on its prefilled input and a runner that answers as the real harness does for a typed case
    const runner = customRunner();
    await renderEditor(runner);
    expect(text('ProblemEditorTestInputHint')).toBe('One JSON value per line: nums, target');

    // When the learner presses Run input
    await pressRunInput();

    // Then the runner got exactly the prefilled case, with expected null, and what it needs to build the values
    await waitFor(() => expect(runner.calls).toHaveLength(1));
    const [call] = runner.calls;
    const problem = ProblemCatalog.getByIdOrThrow('two-sum');
    expect(call?.testCases).toEqual([{ inputs: { nums: [2, 7, 11, 15], target: 9 }, expected: null }]);
    expect(Object.keys(call?.testCases[0]?.inputs ?? {})).toEqual(['nums', 'target']);
    expect(call).toMatchObject({ lang: 'python3', methodName: problem.methodName, paramTypes: problem.paramTypes, returnType: problem.returnType, inPlace: null });
    // And the card shows the input, the result and the time with no expected value, no verdict and no totals
    await waitFor(() => expect(text('ProblemEditorRunOutcomeStatus')).toBe('Custom case'));
    expect(text('ProblemEditorCustomCase-Input')).toBe('nums = [2,7,11,15], target = 9');
    expect(text('ProblemEditorCustomCase-Result')).toBe('[0,1]');
    expect(text('ProblemEditorCustomCase-Time')).toBe('1.5 ms');
    expect(screen.queryByTestId('ProblemEditorRunOutcomeTotals')).toBeNull();
    expect(screen.queryByTestId('ProblemEditorCustomCase-Error')).toBeNull();
    expect(screen.queryByText('Expected')).toBeNull();
    expect(screen.queryByText(/passed|failed/)).toBeNull();
  });

  it('given the learner changes the field to two other values, when Run input is pressed, then the runner receives them in parameter order', async () => {
    const runner = customRunner();
    await renderEditor(runner);

    typeInput('[3,3]\n6');
    await pressRunInput();

    await waitFor(() => expect(runner.calls).toHaveLength(1));
    expect(runner.calls[0]?.testCases).toEqual([{ inputs: { nums: [3, 3], target: 6 }, expected: null }]);
    expect(Object.keys(runner.calls[0]?.testCases[0]?.inputs ?? {})).toEqual(['nums', 'target']);
    await waitFor(() => expect(text('ProblemEditorCustomCase-Input')).toBe('nums = [3,3], target = 6'));
  });

  it('given one value for two parameters, when Run input is pressed, then the error under the field names the parameters, the runner is not asked and no card appears', async () => {
    const runner = customRunner();
    await renderEditor(runner);
    await forgetLoggedLines();

    typeInput('[2,7,11,15]');
    await pressRunInput();

    await waitFor(() => expect(text('ProblemEditorTestInputError')).toBe('Expected 2 values (nums, target), got 1'));
    expect(runner.calls).toHaveLength(0);
    expect(screen.queryByTestId('ProblemEditorRunOutcome')).toBeNull();
    // And the log says which kind of problem it was and nothing the learner typed
    const [line] = await loggedLines('editor_v2.run_local.invalid_input');
    expect(line?.context).toMatchObject({ problemId: 'two-sum', kind: 'count' });
    expect(JSON.stringify(line)).not.toContain('11,15');
  });

  it('given the error under the field, when the learner edits the field, then the error goes away', async () => {
    await renderEditor(customRunner());
    typeInput('[2,7,11,15]');
    await pressRunInput();
    await waitFor(() => expect(screen.getByTestId('ProblemEditorTestInputError')).toBeTruthy());

    typeInput('[2,7,11,15]\n9');

    expect(screen.queryByTestId('ProblemEditorTestInputError')).toBeNull();
  });

  it('given a Python spelling on a line, when Run input is pressed, then the error names the line and says to use null and double quotes, and the runner is not asked', async () => {
    const runner = customRunner();
    await renderEditor(runner);

    typeInput('[2,7,11,15]\nNone');
    await pressRunInput();

    await waitFor(() => expect(text('ProblemEditorTestInputError')).toBe('Line 2 (target) is not JSON: use true, false, null and double quotes'));
    expect(runner.calls).toHaveLength(0);
  });

  it('given curly quotes around a string argument as a phone keyboard types them, when Run input is pressed, then the argument reaches the runner as a string', async () => {
    const runner = customRunner();
    await renderEditor(runner, '/leetcode/valid-anagram');

    typeInput('“anagram”\n“nagaram”');
    await pressRunInput();

    await waitFor(() => expect(runner.calls).toHaveLength(1));
    expect(runner.calls[0]?.testCases).toEqual([{ inputs: { s: 'anagram', t: 'nagaram' }, expected: null }]);
    expect(screen.queryByTestId('ProblemEditorTestInputError')).toBeNull();
  });

  it('given a case that raised, when the card shows, then it reads Error with the message once and a dash for the result', async () => {
    const runner = createFakeCodeRunner((params) => customRun(params, { actual: null, error: 'IndexError: boom', error_at: { line: 3 } }));
    await renderEditor(runner);

    await pressRunInput();

    await waitFor(() => expect(text('ProblemEditorRunOutcomeStatus')).toBe('Error'));
    expect(screen.getAllByText('IndexError: boom')).toHaveLength(1);
    expect(text('ProblemEditorCustomCase-Error')).toBe('IndexError: boom');
    expect(text('ProblemEditorCustomCase-Result')).toBe('—');
    expect(screen.queryByTestId('ProblemEditorRunOutcomeRuntimeError')).toBeNull();
  });

  it('given a case that printed, when the card shows, then the output is on it beside the result', async () => {
    await renderEditor(createFakeCodeRunner((params) => customRun(params, { stdout: 'saw 9\n' })));

    await pressRunInput();

    await waitFor(() => expect(text('ProblemEditorCustomCase-Stdout')).toBe('saw 9'));
    expect(text('ProblemEditorCustomCase-Result')).toBe('[0,1]');
  });

  it('given a code that does not compile, when Run input is pressed, then the card reads Error with the compile error and still echoes the typed input', async () => {
    const compileError: RunResponse = { source: 'offline', kind: 'test', state: 'FAILURE', requestId: '', run_success: false, status_msg: 'Error', total_correct: 0, total_testcases: 0, compile_error: "SyntaxError: expected ':'" };
    await renderEditor(createFakeCodeRunner(() => compileError));

    await pressRunInput();

    await waitFor(() => expect(text('ProblemEditorRunOutcomeStatus')).toBe('Error'));
    expect(text('ProblemEditorCustomCase-Error')).toBe("SyntaxError: expected ':'");
    expect(text('ProblemEditorCustomCase-Input')).toBe('nums = [2,7,11,15], target = 9');
  });

  it('given a runner that rejects, when Run input is pressed, then the card reads Error with its message and the typed field is untouched', async () => {
    expectWarning(/editor_v2\.run_local\.error/);
    const runner = createFakeCodeRunner(() => Promise.reject(new CodeRunnerError('unavailable', 'the runtime files are missing', 'assets')));
    await renderEditor(runner);
    typeInput('[1,2]\n3');

    await pressRunInput();

    await waitFor(() => expect(text('ProblemEditorRunOutcomeStatus')).toBe('Error'));
    expect(text('ProblemEditorCustomCase-Error')).toBe('the runtime files are missing');
    expect(text('ProblemEditorCustomCase-Input')).toBe('nums = [1,2], target = 3');
    expect((screen.getByTestId('ProblemEditorTestInput') as HTMLTextAreaElement).value).toBe('[1,2]\n3');
  });
});

describe('Run input beside Run Local', () => {
  it('given a custom card on screen, when Run Local is pressed, then the one card becomes the tests card with its totals, and Run input brings the custom card back', async () => {
    const runner = createFakeCodeRunner((params) => (params.testCases[0]?.expected === null ? customRun(params) : passingRun(4)));
    await renderEditor(runner);
    await pressRunInput();
    await waitFor(() => expect(text('ProblemEditorRunOutcomeStatus')).toBe('Custom case'));

    await pressRunLocal();

    await waitFor(() => expect(text('ProblemEditorRunOutcomeStatus')).toBe('Accepted'));
    expect(text('ProblemEditorRunOutcomeTotals')).toBe('4 / 4 test cases passed');
    expect(screen.getAllByTestId('ProblemEditorRunOutcome')).toHaveLength(1);
    expect(screen.queryByTestId('ProblemEditorCustomCase')).toBeNull();

    await pressRunInput();

    await waitFor(() => expect(text('ProblemEditorRunOutcomeStatus')).toBe('Custom case'));
    expect(screen.getAllByTestId('ProblemEditorRunOutcome')).toHaveLength(1);
    expect(screen.queryByTestId('ProblemEditorRunOutcomeTotals')).toBeNull();
  });

  it('given a run that is still going, when either button is looked at and tapped again, then both are disabled and the runner is asked once', async () => {
    let finish: (run: RunResponse) => void = () => undefined;
    const runner = createFakeCodeRunner((params) => new Promise<RunResponse>((resolve) => (finish = () => resolve(customRun(params)))));
    await renderEditor(runner);

    await pressRunInput();
    await waitFor(() => expect(runner.calls).toHaveLength(1));

    expect(text('ProblemEditorRunInputButton')).toBe('Running…');
    expect(text('ProblemEditorRunLocalButton')).toBe('Run Local');
    expect(isDisabled('ProblemEditorRunInputButton')).toBe(true);
    expect(isDisabled('ProblemEditorRunLocalButton')).toBe(true);
    strayClick('ProblemEditorRunInputButton');
    strayClick('ProblemEditorRunLocalButton');
    expect(runner.calls).toHaveLength(1);

    finish(customRun(runner.calls[0]!));
    await waitFor(() => expect(text('ProblemEditorRunOutcomeStatus')).toBe('Custom case'));
    expect(isDisabled('ProblemEditorRunInputButton')).toBe(false);
    expect(isDisabled('ProblemEditorRunLocalButton')).toBe(false);
  });

  it('given a custom card on screen, when only the input is edited, then the card stays', async () => {
    await renderEditor(customRunner());
    await pressRunInput();
    await waitFor(() => expect(text('ProblemEditorRunOutcomeStatus')).toBe('Custom case'));

    typeInput('[3,3]\n6');

    expect(text('ProblemEditorCustomCase-Input')).toBe('nums = [2,7,11,15], target = 9');
  });

  it('given a custom card on screen, when the code is edited, then the card goes', async () => {
    const session = await renderEditor(customRunner());
    const editor = await session.editor();
    await pressRunInput();
    await waitFor(() => expect(text('ProblemEditorRunOutcomeStatus')).toBe('Custom case'));

    const edited = `${editor.text()}\n        return [0, 1]`;
    await editor.select(0, editor.text().length);
    await editor.type(edited);

    await waitFor(() => expect(screen.queryByTestId('ProblemEditorRunOutcome')).toBeNull());
  });

  it('given a run that is still going, when the code is edited and the run then finishes, then no card shows over the new code', async () => {
    let finish: () => void = () => undefined;
    const runner = createFakeCodeRunner((params) => new Promise<RunResponse>((resolve) => (finish = () => resolve(customRun(params)))));
    const session = await renderEditor(runner);
    const editor = await session.editor();
    await forgetLoggedLines();
    await pressRunInput();
    await waitFor(() => expect(runner.calls).toHaveLength(1));

    const edited = `${editor.text()}\n        return [0, 1]`;
    await editor.select(0, editor.text().length);
    await editor.type(edited);
    await waitFor(() => expect(editor.text()).toBe(edited));
    // the page's change reaches the screen's state a moment after the page shows it: let it land before the answer does
    await new Promise((resolve) => setTimeout(resolve, 200));
    finish();

    await waitFor(async () => expect(await loggedLines('editor_v2.run_local.discarded')).not.toHaveLength(0), { timeout: 10_000 });
    expect(screen.queryByTestId('ProblemEditorRunOutcome')).toBeNull();
  });
});

describe('Run input without bundled cases', () => {
  it('given a problem with no bundled cases, when its editor opens, then Run input is disabled with the hint and the runner is never asked', async () => {
    // Given a problem whose cases are taken away for this test (a design or class problem has none in the catalog)
    const problem = ProblemCatalog.getByIdOrThrow('two-sum');
    const bundled = problem.testCases;
    problem.testCases = null;
    try {
      const runner = customRunner();
      await renderEditor(runner);

      // When the learner looks at the input zone and taps the button
      expect(text('ProblemEditorTestInputHint')).toBe('No bundled cases for this problem');
      strayClick('ProblemEditorRunInputButton');

      // Then it is disabled and nothing ran
      expect(isDisabled('ProblemEditorRunInputButton')).toBe(true);
      expect(runner.calls).toHaveLength(0);
      expect(screen.queryByTestId('ProblemEditorRunOutcome')).toBeNull();
    } finally {
      problem.testCases = bundled;
    }
  });
});

describe('Run input logging', () => {
  it('given a custom run, when the log is queried, then the result line carries mode custom and neither the code nor an input value appears in any line', async () => {
    const session = await renderEditor(customRunner());
    await session.editor();
    await forgetLoggedLines();
    typeInput('[1234,5678]\n6912');

    await pressRunInput();
    await waitFor(() => expect(text('ProblemEditorRunOutcomeStatus')).toBe('Custom case'));

    const [result] = await loggedLines('editor_v2.run_local.result');
    expect(result?.context).toMatchObject({ mode: 'custom', problemId: 'two-sum' });
    expect(result?.context).not.toHaveProperty('statusMsg');
    expect(result?.context).not.toHaveProperty('totalCorrect');
    const every = JSON.stringify(await loggedLines('editor_v2.run_local'));
    expect(every).not.toContain('5678');
    expect(every).not.toContain('6912');
    expect(every).not.toContain('def twoSum');
  });
});
