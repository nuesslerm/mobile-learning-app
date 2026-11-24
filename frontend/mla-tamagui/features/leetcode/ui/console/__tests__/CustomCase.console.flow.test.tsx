import { act, createFakeAudioRecorder, createFakeVoiceEditTransport, expectWarning, fireEvent, screen, userEvent, waitFor, type FlowSession } from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it } from 'vitest';

import { app } from '../../../../../tests/flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../../tests/flow/loggedLines';
import type { LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../../..';
import { CodeRunnerError } from '../../../domain/ports';
import { ProblemCatalog } from '../../../domain/problemCatalog';
import { createFakeCodeRunner, customRun, passingRun, type FakeCodeRunner } from '../../../testing';
import { exists, fakeGateway, resetConsoleTwin, tap, text, verdict } from './consoleTwinSupport';

async function renderEditor(runner: FakeCodeRunner, route = '/leetcode/two-sum'): Promise<FlowSession> {
  const ports: LeetCodePortOverrides = {
    gateway: fakeGateway(),
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
    runner,
  };
  const session = await app.render(route, { overrides: { leetcode: ports } });
  await session.editor();
  return session;
}

async function openWithCustomCase(): Promise<void> {
  await tap('ConsoleBarOpen');
  await waitFor(() => expect(exists('ConsoleSheet')).toBe(true));
  await tap('ConsoleChipAdd');
}

// fireEvent, not userEvent.type: that reads '[' and '{' as key descriptors
const typeField = (name: string, value: string) => fireEvent.change(screen.getByTestId(`ConsoleField-${name}`), { target: { value } });
const isDisabled = (id: string) => (screen.getByTestId(id) as HTMLButtonElement).disabled || screen.getByTestId(id).getAttribute('aria-disabled') === 'true';
const customRunner = () => createFakeCodeRunner((params) => customRun(params));

beforeEach(() => resetConsoleTwin('two-sum', 'valid-anagram'));

describe('a custom case through the console', () => {
  it('given two-sum open and a custom case copied from Case 1, when it runs, then the runner is asked for that one case with no expected value and the result shows what came back and no verdict', async () => {
    const runner = customRunner();
    await renderEditor(runner);
    await openWithCustomCase();

    await tap('ConsoleFooterRun');

    await waitFor(() => expect(runner.calls).toHaveLength(1));
    const [call] = runner.calls;
    const problem = ProblemCatalog.getByIdOrThrow('two-sum');
    expect(call?.testCases).toEqual([{ inputs: { nums: [2, 7, 11, 15], target: 9 }, expected: null }]);
    expect(Object.keys(call?.testCases[0]?.inputs ?? {})).toEqual(['nums', 'target']);
    expect(call).toMatchObject({ lang: 'python3', methodName: problem.methodName, paramTypes: problem.paramTypes, returnType: problem.returnType, inPlace: null });
    await waitFor(() => expect(verdict()).toBe('Custom case'));
    expect(text('ConsoleResultInput-nums')).toBe('[2,7,11,15]');
    expect(text('ConsoleResultInput-target')).toBe('9');
    expect(text('ConsoleResultOutput')).toBe('[0,1]');
    expect(text('ConsoleResultRuntime')).toBe('Runtime 2 ms');
    expect(exists('ConsoleResultCounts')).toBe(false);
    expect(exists('ConsoleResultError')).toBe(false);
    expect(screen.queryByText('Expected')).toBeNull();
    expect(screen.queryByText(/passed|failed/)).toBeNull();
  });

  it('given the learner changes the fields to two other values, when the case runs, then the runner receives them in parameter order', async () => {
    const runner = customRunner();
    await renderEditor(runner);
    await openWithCustomCase();

    typeField('nums', '[3,3]');
    typeField('target', '6');
    await tap('ConsoleFooterRun');

    await waitFor(() => expect(runner.calls).toHaveLength(1));
    expect(runner.calls[0]?.testCases).toEqual([{ inputs: { nums: [3, 3], target: 6 }, expected: null }]);
    await waitFor(() => expect(text('ConsoleResultInput-nums')).toBe('[3,3]'));
    expect(text('ConsoleResultInput-target')).toBe('6');
  });

  it('given a field that is empty, when the case runs, then the error under the field names it, the runner is not asked, no result appears and the log names the refusal without the typed value', async () => {
    const runner = customRunner();
    await renderEditor(runner);
    await openWithCustomCase();
    await forgetLoggedLines();

    typeField('target', '');
    await tap('ConsoleFooterRun');

    await waitFor(() => expect(text('ConsoleFieldError')).toBe('target is empty'));
    expect(runner.calls).toHaveLength(0);
    expect(exists('ConsoleResultVerdict')).toBe(false);
    const [line] = await loggedLines('editor_v2.console.run_refused');
    expect(line?.context).toMatchObject({ problemId: 'two-sum' });
    expect(JSON.stringify(line)).not.toContain('11,15');
  });

  it('given the error under the field, when the learner edits the field, then the error goes away', async () => {
    await renderEditor(customRunner());
    await openWithCustomCase();
    typeField('target', '');
    await tap('ConsoleFooterRun');
    await waitFor(() => expect(exists('ConsoleFieldError')).toBe(true));

    typeField('target', '9');

    expect(exists('ConsoleFieldError')).toBe(false);
  });

  it('given a Python spelling in a field, when the case runs, then the error names the field and says to use null and double quotes, and the runner is not asked', async () => {
    const runner = customRunner();
    await renderEditor(runner);
    await openWithCustomCase();

    typeField('target', 'None');
    await tap('ConsoleFooterRun');

    await waitFor(() => expect(text('ConsoleFieldError')).toBe('target is not JSON: use true, false, null and double quotes'));
    expect(runner.calls).toHaveLength(0);
  });

  it('given curly quotes around a string argument as a phone keyboard types them, when the case runs, then the argument reaches the runner as a string', async () => {
    const runner = customRunner();
    await renderEditor(runner, '/leetcode/valid-anagram');
    await openWithCustomCase();

    typeField('s', '“anagram”');
    typeField('t', '“nagaram”');
    await tap('ConsoleFooterRun');

    await waitFor(() => expect(runner.calls).toHaveLength(1));
    expect(runner.calls[0]?.testCases).toEqual([{ inputs: { s: 'anagram', t: 'nagaram' }, expected: null }]);
    expect(exists('ConsoleFieldError')).toBe(false);
  });

  it('given a case that raised, when the result shows, then it reads Runtime Error with the message once and no output card', async () => {
    const runner = createFakeCodeRunner((params) => customRun(params, { actual: null, error: 'IndexError: boom', error_at: { line: 3 } }));
    await renderEditor(runner);
    await openWithCustomCase();

    await tap('ConsoleFooterRun');

    await waitFor(() => expect(verdict()).toBe('Runtime Error'));
    expect(screen.getAllByText('IndexError: boom')).toHaveLength(1);
    expect(text('ConsoleResultErrorText')).toBe('IndexError: boom');
    expect(exists('ConsoleResultOutput')).toBe(false);
  });

  it('given a case that printed, when the result shows, then the output is on it beside the result', async () => {
    await renderEditor(createFakeCodeRunner((params) => customRun(params, { stdout: 'saw 9\n' })));
    await openWithCustomCase();

    await tap('ConsoleFooterRun');

    await waitFor(() => expect(text('ConsoleResultStdout')).toContain('saw 9'));
    expect(text('ConsoleResultOutput')).toBe('[0,1]');
  });

  it('given a code that does not compile, when the case runs, then the result reads Compile Error with the message', async () => {
    const compileError: RunResponse = { source: 'offline', kind: 'test', state: 'FAILURE', requestId: '', run_success: false, status_msg: 'Error', total_correct: 0, total_testcases: 0, compile_error: "SyntaxError: expected ':'" };
    await renderEditor(createFakeCodeRunner(() => compileError));
    await openWithCustomCase();

    await tap('ConsoleFooterRun');

    await waitFor(() => expect(verdict()).toBe('Compile Error'));
    expect(text('ConsoleResultErrorText')).toBe("SyntaxError: expected ':'");
  });

  it('given a runner that rejects, when the case runs, then the result reads Run failed with its message and the typed fields are untouched', async () => {
    expectWarning(/editor_v2\.run_local\.error/);
    await renderEditor(createFakeCodeRunner(() => Promise.reject(new CodeRunnerError('unavailable', 'the runtime files are missing', 'assets'))));
    await openWithCustomCase();
    typeField('nums', '[1,2]');
    typeField('target', '3');

    await tap('ConsoleFooterRun');

    await waitFor(() => expect(verdict()).toBe('Run failed'));
    expect(text('ConsoleResultErrorText')).toBe('the runtime files are missing');
    await tap('ConsoleTabs-testcase');
    expect((screen.getByTestId('ConsoleField-nums') as HTMLTextAreaElement).value).toBe('[1,2]');
    expect((screen.getByTestId('ConsoleField-target') as HTMLTextAreaElement).value).toBe('3');
  });
});

describe('a custom case beside Submit', () => {
  it('given a custom result on screen, when Submit is tapped, then the one result becomes the tests result with its tally, and Run brings the custom result back', async () => {
    const runner = createFakeCodeRunner((params) => (params.testCases[0]?.expected === null ? customRun(params) : passingRun(4)));
    await renderEditor(runner);
    await openWithCustomCase();
    await tap('ConsoleFooterRun');
    await waitFor(() => expect(verdict()).toBe('Custom case'));

    await tap('ConsoleFooterSubmit');

    await waitFor(() => expect(verdict()).toBe('Accepted'));
    expect(text('ConsoleResultCounts')).toBe('4 / 4 testcases passed');
    expect(screen.getAllByTestId('ConsoleResultVerdict')).toHaveLength(1);

    await tap('ConsoleFooterRun');

    await waitFor(() => expect(verdict()).toBe('Custom case'));
    expect(screen.getAllByTestId('ConsoleResultVerdict')).toHaveLength(1);
    expect(exists('ConsoleResultCounts')).toBe(false);
  });

  it('given a run that is still going, when the buttons are looked at and tapped again, then they are disabled and the runner is asked once', async () => {
    let finish: () => void = () => undefined;
    const runner = createFakeCodeRunner((params) => new Promise<RunResponse>((resolve) => (finish = () => resolve(customRun(params)))));
    await renderEditor(runner);
    await openWithCustomCase();

    await tap('ConsoleFooterRun');
    await waitFor(() => expect(runner.calls).toHaveLength(1));

    await waitFor(() => {
      expect(isDisabled('ConsoleFooterRun')).toBe(true);
      expect(isDisabled('ConsoleFooterSubmit')).toBe(true);
    });
    fireEvent.click(screen.getByTestId('ConsoleFooterRun'));
    fireEvent.click(screen.getByTestId('ConsoleFooterSubmit'));
    expect(runner.calls).toHaveLength(1);

    await act(async () => finish());
    await waitFor(() => expect(verdict()).toBe('Custom case'));
    await waitFor(() => {
      expect(isDisabled('ConsoleFooterRun')).toBe(false);
      expect(isDisabled('ConsoleFooterSubmit')).toBe(false);
    });
  });

  it('given a custom result on screen, when only a field is edited, then the result stays', async () => {
    await renderEditor(customRunner());
    await openWithCustomCase();
    await tap('ConsoleFooterRun');
    await waitFor(() => expect(verdict()).toBe('Custom case'));

    await tap('ConsoleTabs-testcase');
    typeField('nums', '[3,3]');
    await tap('ConsoleTabs-result');

    expect(text('ConsoleResultInput-nums')).toBe('[2,7,11,15]');
  });

  it('given a custom result on screen, when the code is edited, then the result goes', async () => {
    const session = await renderEditor(customRunner());
    const editor = await session.editor();
    await openWithCustomCase();
    await tap('ConsoleFooterRun');
    await waitFor(() => expect(verdict()).toBe('Custom case'));

    const edited = `${editor.text()}\n        return [0, 1]`;
    await editor.select(0, editor.text().length);
    await editor.type(edited);

    await waitFor(() => expect(exists('ConsoleResultVerdict')).toBe(false));
  });

  it('given a run that is still going, when the code is edited and the run then finishes, then no result is left over the new code', async () => {
    let finish: () => void = () => undefined;
    const runner = createFakeCodeRunner((params) => new Promise<RunResponse>((resolve) => (finish = () => resolve(customRun(params)))));
    const session = await renderEditor(runner);
    const editor = await session.editor();
    await openWithCustomCase();
    await tap('ConsoleFooterRun');
    await waitFor(() => expect(runner.calls).toHaveLength(1));

    const edited = `${editor.text()}\n        return [0, 1]`;
    await forgetLoggedLines();
    await editor.select(0, editor.text().length);
    await editor.type(edited);
    await waitFor(() => expect(editor.text()).toBe(edited));
    await waitFor(async () => expect(await loggedLines('editor.value_sync')).not.toHaveLength(0));
    await act(async () => finish());

    await waitFor(() => expect(exists('ConsolePending')).toBe(false));
    await waitFor(async () => expect(await loggedLines('editor_v2.run_local.discarded')).not.toHaveLength(0));
    expect(exists('ConsoleResultVerdict')).toBe(false);
  });
});

describe('the case fields', () => {
  it('given the sheet open, when the learner taps a field, then it keeps the focus and the page does not take the tap for one away', async () => {
    await renderEditor(customRunner());
    await openWithCustomCase();
    await forgetLoggedLines();
    const field = screen.getByTestId('ConsoleField-nums');

    await userEvent.click(field);

    expect(document.activeElement).toBe(field);
    expect(await loggedLines('editor_v2.keyboard_dismiss')).toHaveLength(0);
  });
});

describe('a custom case without bundled cases', () => {
  it('given a problem with no bundled cases, when the console opens, then it says so, Run is disabled and the runner is never asked', async () => {
    const problem = ProblemCatalog.getByIdOrThrow('two-sum');
    const bundled = problem.testCases;
    problem.testCases = null;
    try {
      const runner = customRunner();
      await renderEditor(runner);
      await tap('ConsoleBarOpen');
      await waitFor(() => expect(exists('ConsoleSheet')).toBe(true));

      expect(exists('ConsoleNoCases')).toBe(true);
      fireEvent.click(screen.getByTestId('ConsoleFooterRun'));

      expect(isDisabled('ConsoleFooterRun')).toBe(true);
      expect(runner.calls).toHaveLength(0);
      expect(exists('ConsoleResultVerdict')).toBe(false);
    } finally {
      problem.testCases = bundled;
    }
  });
});

describe('a custom run in the log', () => {
  it('given a custom run, when the log is queried, then the run-end line says a custom case ended and neither the code nor an input value appears in any line', async () => {
    const session = await renderEditor(customRunner());
    await session.editor();
    await openWithCustomCase();
    await forgetLoggedLines();
    typeField('nums', '[1234,5678]');
    typeField('target', '6912');

    await tap('ConsoleFooterRun');
    await waitFor(() => expect(verdict()).toBe('Custom case'));

    const [result] = await loggedLines('editor_v2.console.run_end');
    expect(result?.context).toMatchObject({ mode: 'run', verdict: 'custom', problemId: 'two-sum' });
    const every = JSON.stringify(await loggedLines('editor_v2.run_local')) + JSON.stringify(await loggedLines('editor_v2.console'));
    expect(every).not.toContain('5678');
    expect(every).not.toContain('6912');
    expect(every).not.toContain('def twoSum');
  });
});
