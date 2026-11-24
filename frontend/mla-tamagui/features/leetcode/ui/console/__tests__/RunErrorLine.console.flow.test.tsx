import { act, createFakeAudioRecorder, createFakeVoiceEditTransport, routerHarness, screen, setStackHeadersRendered, userEvent, waitFor, within, type FlowCodeEditor, type FlowSession } from '@lib/flow-test-harness';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { app } from '../../../../../tests/flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../../tests/flow/loggedLines';
import type { CodeRunnerPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../../..';
import { createPyodideRunner, type PyodideRunner, type RunnerLimits } from '../../../data/pyodide/pyodideRunner';
import type { EditorBridgePort } from '../../../domain/ports';
import { useResetCodeRequestStore } from '../../../state/resetCodeRequestStore';
import { useRunnerSettingStore } from '../../../state/runnerSettingStore';
import { answers, createFakeCodeRunner, createFakePageHost, editorBridgeWith, failingRun, type FakePageHost, type FakePageScript } from '../../../testing';
import { exists, fakeGateway, resetConsoleTwin, tap, text, verdict } from './consoleTwinSupport';

// line 2 is the starter's own signature, so a reset leaves it in the part of the text that does not change
const SEED = ['class Solution:', '    def twoSum(self, nums: List[int], target: int) -> List[int]:', '        seen = {}', '        value = seen[target]', '        pairs = [value]', '        return pairs'].join('\n');
const LINE_2 = 'def twoSum(self, nums: List[int], target: int) -> List[int]:';
const LINE_4 = 'value = seen[target]';
const LINE_6 = 'return pairs';

const LIMITS: Partial<RunnerLimits> = { readyMs: 2000, pullMs: 2000, bootMs: 2000, waitMs: 2000, runMs: 250, rpcSlackMs: 100 };
const RAISED_ROW = { pass: false, input: { nums: [2, 7, 11, 15], target: 9 }, actual: null, expected: [0, 1], stdout: '', stderr: '', error: 'KeyError: 9', error_at: { line: 4 }, elapsed_ms: 1 };
const FAILED_AT_LINE_4: RunResponse = { ...failingRun(3), results: [RAISED_ROW], runtime_error: 'KeyError: 9', error_at: { line: 4 } };

function deviceRunner(script: FakePageScript = {}): { host: FakePageHost; runner: PyodideRunner } {
  const host = createFakePageHost({ readyAfterMs: 5, ...script });
  return { host, runner: createPyodideRunner({ host, limits: LIMITS }) };
}

async function renderEditor(options: { runner?: CodeRunnerPort; deviceRunner?: CodeRunnerPort; editorBridge?: EditorBridgePort } = {}): Promise<FlowSession> {
  const ports: LeetCodePortOverrides = {
    gateway: fakeGateway(),
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
    ...(options.runner ? { runner: options.runner } : {}),
    ...(options.deviceRunner ? { deviceRunner: options.deviceRunner } : {}),
    ...(options.editorBridge ? { editorBridge: options.editorBridge } : {}),
  };
  return app.render('/leetcode/two-sum', { overrides: { leetcode: ports }, search: { seed: SEED } });
}

const pressRun = () => tap('ConsoleBarRun');
const pressSubmit = () => tap('ConsoleBarSubmit');
const cardIsGone = () => expect(exists('ConsoleResultVerdict')).toBe(false);

/** The text each error mark covers in what the editor holds now. */
async function markedTexts(editor: FlowCodeEditor): Promise<string[]> {
  const code = editor.text();
  return (await editor.marks()).map((mark) => code.slice(mark.range.from, mark.range.to));
}

/** What CodeMirror draws as the error, which it may split at its syntax colours: the pieces joined are the line's content. */
const drawnError = (editor: FlowCodeEditor) => editor.errorLines().join('');

const loggedText = async (event: string) => JSON.stringify(await loggedLines(event));
const valueSyncs = async () => (await loggedLines('editor.value_sync')).length;

beforeEach(() => resetConsoleTwin('two-sum'));
afterEach(() => setStackHeadersRendered(false));

describe('a failing run on the device marks the line it reports, through the console', () => {
  it('given the device runner answers a case error at line 4 of a six-line solution, when the user taps Submit, then line 4 is marked and revealed with the caret and focus left alone, and the result shows the error and its line', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { runner } = deviceRunner({ judge: (call) => answers.caseError(call, 'KeyError: 9', { line: 4 }, 1) });
    const session = await renderEditor({ deviceRunner: runner });
    const editor = await session.editor();

    await pressSubmit();

    await waitFor(() => expect(text('ConsoleResultErrorText')).toContain('KeyError: 9'));
    await waitFor(async () => expect(await markedTexts(editor)).toEqual([LINE_4]));
    await waitFor(() => expect(drawnError(editor)).toBe(LINE_4));
    expect(await editor.revealedLine()).toBe(4);
    expect(verdict()).toBe('Runtime Error');
    expect(text('ConsoleResultLine')).toBe('Line 4 (Solution.py)');
    expect(editor.selection()).toEqual({ start: 0, end: 0 });
    expect(editor.focused()).toBe(false);
    expect(editor.text()).toBe(SEED);
    expect(await loggedText('editor_v2.run_error.shown')).toContain('"line":4');
  });

  it('given the device runner answers a compile error at line 2, when the user taps Run, then line 2 is marked', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { runner } = deviceRunner({ judge: () => answers.compileError("CompileError: expected ':'", { line: 2, column: 5 }) });
    const session = await renderEditor({ deviceRunner: runner });
    const editor = await session.editor();

    await pressRun();

    await waitFor(() => expect(text('ConsoleResultErrorText')).toContain("expected ':'"));
    await waitFor(async () => expect(await markedTexts(editor)).toEqual([LINE_2]));
    expect(await editor.revealedLine()).toBe(2);
  });

  it('given the page raises at line 4 for a custom case, when it runs, then the result shows the error and line 4 is marked: the same jump as the tests', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { runner } = deviceRunner({ judge: (call) => answers.caseError(call, 'KeyError: 9', { line: 4 }) });
    const session = await renderEditor({ deviceRunner: runner });
    const editor = await session.editor();
    await tap('ConsoleBarOpen');
    await waitFor(() => expect(exists('ConsoleSheet')).toBe(true));
    await tap('ConsoleChipAdd');

    await tap('ConsoleFooterRun');

    await waitFor(() => expect(verdict()).toBe('Runtime Error'));
    await waitFor(async () => expect(await markedTexts(editor)).toEqual([LINE_4]));
    expect(await editor.revealedLine()).toBe(4);
  });

  it('given a result with no line, when the user taps Submit, then nothing is marked or revealed and the result is shown', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { runner } = deviceRunner({ judge: (call) => answers.wrongAnswer(call, 3) });
    const session = await renderEditor({ deviceRunner: runner });
    const editor = await session.editor();

    await pressSubmit();

    await waitFor(() => expect(verdict()).toBe('Wrong Answer'));
    expect(await editor.marks()).toEqual([]);
    expect(await editor.revealedLine()).toBeNull();
    expect(editor.errorLines()).toEqual([]);
  });

  it('given a result whose line is past the end of the code, when the run finishes, then nothing is marked, the result is shown and the log says outside', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { runner } = deviceRunner({ judge: (call) => answers.caseError(call, 'KeyError: 9', { line: 40 }, 1) });
    const session = await renderEditor({ deviceRunner: runner });
    const editor = await session.editor();
    await forgetLoggedLines();

    await pressSubmit();

    await waitFor(() => expect(text('ConsoleResultErrorText')).toContain('KeyError: 9'));
    await waitFor(async () => expect(await loggedText('editor_v2.run_error.skipped')).toContain('outside'));
    expect(await editor.marks()).toEqual([]);
    expect(await editor.revealedLine()).toBeNull();
  });

  it('given a mark on line 4, when the user runs again and the new error is on line 6, then exactly one mark exists, on line 6', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { host, runner } = deviceRunner({ judge: (call) => answers.caseError(call, 'KeyError: 9', { line: 4 }, 1) });
    const session = await renderEditor({ deviceRunner: runner });
    const editor = await session.editor();
    await pressSubmit();
    await waitFor(async () => expect(await markedTexts(editor)).toEqual([LINE_4]));

    host.script({ judge: (call) => answers.caseError(call, 'NameError: pairs', { line: 6 }, 1) });
    await tap('ConsoleFooterSubmit');

    await waitFor(async () => expect(await markedTexts(editor)).toEqual([LINE_6]));
    expect(await editor.revealedLine()).toBe(6);
  });
});

describe('the mark lives as long as the result, through the console', () => {
  async function failedRun() {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { runner } = deviceRunner({ judge: () => answers.compileError('CompileError: invalid syntax', { line: 2 }) });
    const session = await renderEditor({ deviceRunner: runner });
    const editor = await session.editor();
    await pressRun();
    await waitFor(async () => expect(await markedTexts(editor)).toEqual([LINE_2]));
    return { session, editor };
  }

  it('given a mark is shown, when the user types a character, then the result and the mark are both gone', async () => {
    const { editor } = await failedRun();

    await editor.type('x');

    await waitFor(() => expect(exists('ConsoleResultEmpty')).toBe(true));
    cardIsGone();
    await waitFor(async () => expect(await editor.marks()).toEqual([]));
    expect(editor.errorLines()).toEqual([]);
  });

  it('given the result goes while the mark is still on its way to the editor, when the mark lands, then it is taken away at once', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { runner } = deviceRunner({ judge: (call) => answers.caseError(call, 'KeyError: 9', { line: 4 }, 1) });
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => (release = resolve));
    const held = editorBridgeWith((real) => ({
      setMarks: async (specs, options) => {
        await gate;
        return real.setMarks(specs, options);
      },
    }));
    const session = await renderEditor({ deviceRunner: runner, editorBridge: held });
    const editor = await session.editor();
    await pressSubmit();
    await waitFor(() => expect(verdict()).toBe('Runtime Error'));

    await editor.type('x');
    await waitFor(() => expect(exists('ConsoleResultEmpty')).toBe(true));
    release();

    await waitFor(async () => expect(await loggedText('editor_v2.run_error.shown')).toContain('"line":4'));
    await waitFor(async () => expect(await editor.marks()).toEqual([]));
    expect(editor.errorLines()).toEqual([]);
  });

  it('given a mark is shown, when the user resets the code through the header menu, then the result and the mark are both gone', async () => {
    const r = routerHarness();
    setStackHeadersRendered(true);
    const { editor } = await failedRun();

    await tap('ProblemEditorMenuButton');
    await waitFor(() => expect(screen.getByTestId('ProblemEditorMenuButtonContent')).toBeTruthy());
    await userEvent.click(within(screen.getByTestId('ProblemEditorMenuButtonContent')).getByTestId('ProblemEditorMenuResetCode'));
    await waitFor(() => expect(r.lastPush()).toBe('/leetcode/two-sum/reset-code'));
    await act(async () => useResetCodeRequestStore.getState().payload?.onConfirm());

    await waitFor(async () => expect(await editor.marks()).toEqual([]));
    expect(editor.errorLines()).toEqual([]);
    await waitFor(() => expect(exists('ConsoleResultVerdict')).toBe(false));
  });

  it('given a mark is shown, when the user switches the language, then the result and the mark are both gone', async () => {
    const { editor } = await failedRun();

    await userEvent.click(screen.getByTestId('ProblemEditorLanguagePicker-Trigger'));
    await waitFor(() => expect(screen.getByTestId('ProblemEditorLanguagePicker-JavaScript')).toBeTruthy());
    await userEvent.click(screen.getByTestId('ProblemEditorLanguagePicker-JavaScript'));

    await waitFor(async () => expect(await editor.marks()).toEqual([]));
    expect(editor.errorLines()).toEqual([]);
    await waitFor(() => expect(exists('ConsoleResultVerdict')).toBe(false));
  });
});

describe('the line in the result takes the person to the editor', () => {
  async function runWith(judge: FakePageScript['judge'], options: { editorBridge?: EditorBridgePort } = {}) {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { runner } = deviceRunner({ judge });
    const session = await renderEditor({ deviceRunner: runner, ...options });
    const editor = await session.editor();
    await forgetLoggedLines();
    return { editor };
  }

  it('given a located error and the result showing Line 4, when the user taps it, then the sheet closes, the editor reveals line 4 and the mark stays', async () => {
    const { editor } = await runWith((call) => answers.caseError(call, 'KeyError: 9', { line: 4 }, 1));
    await pressSubmit();
    await waitFor(() => expect(screen.getByTestId('ConsoleResultLine').tagName).toBe('BUTTON'));
    expect(text('ConsoleResultLine')).toBe('Line 4 (Solution.py)');
    await forgetLoggedLines();

    await tap('ConsoleResultLine');

    await waitFor(() => expect(exists('ConsoleSheet')).toBe(false));
    await waitFor(async () => expect(await loggedText('editor_v2.run_error.shown')).toContain('"line":4'));
    expect(await editor.revealedLine()).toBe(4);
    expect(await markedTexts(editor)).toEqual([LINE_4]);
  });

  it('given the editor refused the mark, when the result shows, then Line N is offered as text and not as a button', async () => {
    const reportsAnotherPage = editorBridgeWith((real) => ({
      hello: async () => {
        const reply = await real.hello();
        return reply.ok ? { ...reply, value: { ...reply.value, instance: 'page-before-the-restart' } } : reply;
      },
    }));
    await runWith((call) => answers.caseError(call, 'KeyError: 9', { line: 4 }, 1), { editorBridge: reportsAnotherPage });

    await pressSubmit();

    await waitFor(async () => expect(await loggedText('editor_v2.run_error.refused')).toContain('instance-mismatch'));
    expect(screen.getByTestId('ConsoleResultLine').tagName).not.toBe('BUTTON');
  });

  it('given a located error was shown, when the user types so the result goes, then no Line N is offered', async () => {
    const { editor } = await runWith((call) => answers.caseError(call, 'KeyError: 9', { line: 4 }, 1));
    await pressSubmit();
    await screen.findByTestId('ConsoleResultLine');

    await editor.type('x');

    await waitFor(() => expect(exists('ConsoleResultEmpty')).toBe(true));
    expect(exists('ConsoleResultLine')).toBe(false);
  });

  it('given a run without a line, when the result is shown, then no button for Line N is offered', async () => {
    await runWith((call) => answers.wrongAnswer(call, 3));

    await pressSubmit();

    await waitFor(() => expect(verdict()).toBe('Wrong Answer'));
    expect(exists('ConsoleResultLine')).toBe(false);
  });
});

describe('a run that goes on while the person types, through the console', () => {
  function heldRun() {
    let finish: (run: RunResponse) => void = () => undefined;
    const runner = createFakeCodeRunner(() => new Promise<RunResponse>((resolve) => (finish = resolve)));
    return { runner, finish: (run: RunResponse) => act(async () => finish(run)) };
  }

  it('given the run is going, when the user types a character and removes it and the run finishes at line 4, then the result is shown and the mark is on line 4', async () => {
    const { runner, finish } = heldRun();
    const session = await renderEditor({ runner });
    const editor = await session.editor();
    await pressSubmit();
    await waitFor(() => expect(runner.calls).toHaveLength(1));

    await forgetLoggedLines();
    await editor.type('x');
    await waitFor(async () => expect(await valueSyncs()).toBeGreaterThanOrEqual(1));
    await editor.select(0, 1);
    await editor.type('');
    await waitFor(async () => expect(await valueSyncs()).toBeGreaterThanOrEqual(2));
    expect(editor.text()).toBe(SEED);
    await finish(FAILED_AT_LINE_4);

    await waitFor(() => expect(verdict()).toBe('Runtime Error'));
    await waitFor(async () => expect(await markedTexts(editor)).toEqual([LINE_4]));
  });

  it('given the run is going, when the user types and the run finishes, then the verdict is dropped and nothing is marked', async () => {
    const { runner, finish } = heldRun();
    const session = await renderEditor({ runner });
    const editor = await session.editor();
    await pressSubmit();
    await waitFor(() => expect(runner.calls).toHaveLength(1));

    await forgetLoggedLines();
    await editor.type('x');
    await waitFor(async () => expect(await valueSyncs()).toBeGreaterThanOrEqual(1));
    await finish(FAILED_AT_LINE_4);

    await waitFor(async () => expect((await loggedLines('editor_v2.run_local.discarded')).length).toBeGreaterThan(0));
    cardIsGone();
    expect(await editor.marks()).toEqual([]);
    expect(editor.errorLines()).toEqual([]);
  });
});

describe('an editor that cannot give its text, through the console', () => {
  it('given an editor that never answers, when the user taps Submit, then the run goes ahead on the screen\'s text after the deadline, the result is shown and no line is marked', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { host, runner } = deviceRunner({ judge: (call) => answers.caseError(call, 'KeyError: 9', { line: 4 }, 1) });
    const session = await renderEditor({ deviceRunner: runner, editorBridge: editorBridgeWith(() => ({ hello: () => new Promise<never>(() => undefined) })) });
    const editor = await session.editor();
    await forgetLoggedLines();

    await pressSubmit();

    await waitFor(() => expect(text('ConsoleResultErrorText')).toContain('KeyError: 9'), { timeout: 10_000 });
    const judged = host.calls.filter((call) => call.method === 'judge').map((call) => call.params as { payload: { code: string } });
    expect(judged[0]?.payload.code).toBe(SEED);
    expect(await loggedText('editor_v2.run_error.skipped')).toContain('no-checkpoint');
    expect(await editor.marks()).toEqual([]);
    expect(await editor.revealedLine()).toBeNull();
  });

  it('given the editor that was read at the run start is not the one that answers when the verdict arrives, when the run finishes, then nothing is marked and the log says refused', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { runner } = deviceRunner({ judge: (call) => answers.caseError(call, 'KeyError: 9', { line: 4 }, 1) });
    const reportsAnotherPage = editorBridgeWith((real) => ({
      hello: async () => {
        const reply = await real.hello();
        return reply.ok ? { ...reply, value: { ...reply.value, instance: 'page-before-the-restart' } } : reply;
      },
    }));
    const session = await renderEditor({ deviceRunner: runner, editorBridge: reportsAnotherPage });
    const editor = await session.editor();
    await forgetLoggedLines();

    await pressSubmit();

    await waitFor(() => expect(text('ConsoleResultErrorText')).toContain('KeyError: 9'));
    await waitFor(async () => expect(await loggedText('editor_v2.run_error.refused')).toContain('instance-mismatch'));
    expect(await editor.marks()).toEqual([]);
    expect(editor.errorLines()).toEqual([]);
  });
});
