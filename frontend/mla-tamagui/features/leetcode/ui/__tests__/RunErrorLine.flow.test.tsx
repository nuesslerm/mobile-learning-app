import { act, createFakeAudioRecorder, createFakeVoiceEditTransport, routerHarness, screen, userEvent, waitFor, type FlowCodeEditor, type FlowSession } from '@lib/flow-test-harness';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../tests/flow/loggedLines';
import type { CodeRunnerPort, LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';
import type { EditorBridgePort } from '../../domain/ports';
import { createPyodideRunner, type PyodideRunner, type RunnerLimits } from '../../data/pyodide/pyodideRunner';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import { useResetCodeRequestStore } from '../../state/resetCodeRequestStore';
import { answers, createFakeCodeRunner, createFakePageHost, editorBridgeWith, failingRun, type FakePageHost, type FakePageScript } from '../../testing';

// line 2 is the starter's own signature, so a reset leaves it in the part of the text that does not change
const SEED = ['class Solution:', '    def twoSum(self, nums: List[int], target: int) -> List[int]:', '        seen = {}', '        value = seen[target]', '        pairs = [value]', '        return pairs'].join('\n');
const LINE_2 = 'def twoSum(self, nums: List[int], target: int) -> List[int]:';
const LINE_4 = 'value = seen[target]';
const LINE_6 = 'return pairs';

const BACKEND_RUN: RunResponse = { source: 'backend-run', kind: 'test', state: 'SUCCESS', requestId: 'backend', run_success: true, status_msg: 'Accepted', total_correct: 5, total_testcases: 5 };
const LIMITS: Partial<RunnerLimits> = { readyMs: 2000, pullMs: 2000, bootMs: 2000, waitMs: 2000, runMs: 250, rpcSlackMs: 100 };
const FAILED_AT_LINE_4: RunResponse = { ...failingRun(3), runtime_error: 'KeyError: 9', error_at: { line: 4 } };

function deviceRunner(script: FakePageScript = {}): { host: FakePageHost; runner: PyodideRunner } {
  const host = createFakePageHost({ readyAfterMs: 5, ...script });
  return { host, runner: createPyodideRunner({ host, limits: LIMITS }) };
}

async function renderEditor(options: { runner?: CodeRunnerPort; deviceRunner?: CodeRunnerPort; editorBridge?: EditorBridgePort; seed?: string } = {}): Promise<FlowSession> {
  const gateway: LeetCodeGatewayPort = {
    userStatus: vi.fn(async () => ({ isSignedIn: true, username: 'flow-user' })),
    test: vi.fn(async () => BACKEND_RUN),
    submit: vi.fn(async () => BACKEND_RUN),
    poll: vi.fn(async () => BACKEND_RUN),
    runLocal: vi.fn(async () => BACKEND_RUN),
    format: vi.fn(async (p: { lang: string; code: string }) => p.code),
    loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })),
  };
  const ports: LeetCodePortOverrides = {
    gateway,
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
    ...(options.runner ? { runner: options.runner } : {}),
    ...(options.deviceRunner ? { deviceRunner: options.deviceRunner } : {}),
    ...(options.editorBridge ? { editorBridge: options.editorBridge } : {}),
  };
  return app.render('/leetcode/two-sum', { overrides: { leetcode: ports }, search: { seed: options.seed ?? SEED } });
}

const pressRun = () => userEvent.click(screen.getByTestId('ProblemEditorRunLocalButton'));
const pressRunInput = () => userEvent.click(screen.getByTestId('ProblemEditorRunInputButton'));
const status = () => screen.getByTestId('ProblemEditorRunOutcomeStatus').textContent;
const cardIsGone = () => expect(screen.queryByTestId('ProblemEditorRunOutcomeStatus')).toBeNull();

/** The text each error mark covers in what the editor holds now. */
async function markedTexts(editor: FlowCodeEditor): Promise<string[]> {
  const text = editor.text();
  return (await editor.marks()).map((mark) => text.slice(mark.range.from, mark.range.to));
}

/** What CodeMirror draws as the error, which it may split at its syntax colours: the pieces joined are the line's content. */
const drawnError = (editor: FlowCodeEditor) => editor.errorLines().join('');

const loggedText = async (event: string) => JSON.stringify(await loggedLines(event));
const valueSyncs = async () => (await loggedLines('editor.value_sync')).length;

afterEach(() => useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER }));

describe('a failing run on the device marks the line it reports', () => {
  it('given the device runner answers a case error at line 4 of a six-line solution, when the user runs the tests, then line 4 is marked and revealed with the caret and focus left alone, and the card still shows the error', async () => {
    // Given the editor on a six-line solution and a page whose second case raises at line 4
    useRunnerSettingStore.setState({ runner: 'device' });
    const { runner } = deviceRunner({ judge: (call) => answers.caseError(call, 'KeyError: 9', { line: 4 }, 1) });
    const session = await renderEditor({ deviceRunner: runner });
    const editor = await session.editor();

    // When the user presses Run Local
    await pressRun();

    // Then the card shows the error, one error mark covers the content of line 4 and is drawn and revealed
    await waitFor(() => expect(screen.getByTestId('ProblemEditorRunOutcomeRuntimeError').textContent).toContain('KeyError: 9'));
    await waitFor(async () => expect(await markedTexts(editor)).toEqual([LINE_4]));
    await waitFor(() => expect(drawnError(editor)).toBe(LINE_4));
    expect(await editor.revealedLine()).toBe(4);
    expect(status()).toBe('Wrong Answer');

    // And the jump moved neither the caret nor the focus (the keyboard stays down), and the text is the one that ran
    expect(editor.selection()).toEqual({ start: 0, end: 0 });
    expect(editor.focused()).toBe(false);
    expect(editor.text()).toBe(SEED);
    expect(await loggedText('editor_v2.run_error.shown')).toContain('"line":4');
  });

  it('given the device runner answers a compile error at line 2, when the user runs the tests, then line 2 is marked', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { runner } = deviceRunner({ judge: () => answers.compileError("CompileError: expected ':'", { line: 2, column: 5 }) });
    const session = await renderEditor({ deviceRunner: runner });
    const editor = await session.editor();

    await pressRun();

    await waitFor(() => expect(screen.getByTestId('ProblemEditorRunOutcomeCompileError').textContent).toContain("expected ':'"));
    await waitFor(async () => expect(await markedTexts(editor)).toEqual([LINE_2]));
    expect(await editor.revealedLine()).toBe(2);
  });

  it('given the page raises at line 4 for a typed case, when the learner presses Run input, then the card shows the error and line 4 is marked: the same jump as the tests', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { runner } = deviceRunner({ judge: (call) => answers.caseError(call, 'KeyError: 9', { line: 4 }) });
    const session = await renderEditor({ deviceRunner: runner });
    const editor = await session.editor();

    await pressRunInput();

    await waitFor(() => expect(status()).toBe('Error'));
    await waitFor(async () => expect(await markedTexts(editor)).toEqual([LINE_4]));
    expect(await editor.revealedLine()).toBe(4);
  });

  it('given a result with no line, when the user runs the tests, then nothing is marked or revealed and the card is shown', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { runner } = deviceRunner({ judge: (call) => answers.wrongAnswer(call, 3) });
    const session = await renderEditor({ deviceRunner: runner });
    const editor = await session.editor();
    await forgetLoggedLines();

    await pressRun();

    await waitFor(() => expect(status()).toBe('Wrong Answer'));
    await waitFor(async () => expect(await loggedText('editor_v2.run_error.skipped')).toContain('no-location'));
    expect(await editor.marks()).toEqual([]);
    expect(await editor.revealedLine()).toBeNull();
    expect(editor.errorLines()).toEqual([]);
  });

  it('given a result whose line is past the end of the code, when the run finishes, then nothing is marked, the card is shown and the log says outside', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { runner } = deviceRunner({ judge: (call) => answers.caseError(call, 'KeyError: 9', { line: 40 }, 1) });
    const session = await renderEditor({ deviceRunner: runner });
    const editor = await session.editor();
    await forgetLoggedLines();

    await pressRun();

    await waitFor(() => expect(screen.getByTestId('ProblemEditorRunOutcomeRuntimeError').textContent).toContain('KeyError: 9'));
    await waitFor(async () => expect(await loggedText('editor_v2.run_error.skipped')).toContain('outside'));
    expect(await editor.marks()).toEqual([]);
    expect(await editor.revealedLine()).toBeNull();
  });

  it('given a mark on line 4, when the user runs again and the new error is on line 6, then exactly one mark exists, on line 6', async () => {
    // Given a first run that raised at line 4 and left its mark
    useRunnerSettingStore.setState({ runner: 'device' });
    const { host, runner } = deviceRunner({ judge: (call) => answers.caseError(call, 'KeyError: 9', { line: 4 }, 1) });
    const session = await renderEditor({ deviceRunner: runner });
    const editor = await session.editor();
    await pressRun();
    await waitFor(async () => expect(await markedTexts(editor)).toEqual([LINE_4]));

    // When the page answers the next run with an error at line 6
    host.script({ judge: (call) => answers.caseError(call, 'NameError: pairs', { line: 6 }, 1) });
    await pressRun();

    // Then the one mark is on line 6
    await waitFor(async () => expect(await markedTexts(editor)).toEqual([LINE_6]));
    expect(await editor.revealedLine()).toBe(6);
  });
});

describe('the mark lives as long as the card', () => {
  async function failedRun() {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { runner } = deviceRunner({ judge: () => answers.compileError('CompileError: invalid syntax', { line: 2 }) });
    const session = await renderEditor({ deviceRunner: runner });
    const editor = await session.editor();
    await pressRun();
    await waitFor(async () => expect(await markedTexts(editor)).toEqual([LINE_2]));
    return { session, editor };
  }

  it('given a mark is shown, when the user types a character, then the card and the mark are both gone', async () => {
    const { editor } = await failedRun();

    await editor.type('x');

    await waitFor(cardIsGone);
    await waitFor(async () => expect(await editor.marks()).toEqual([]));
    expect(editor.errorLines()).toEqual([]);
  });

  it('given the card goes while the mark is still on its way to the editor, when the mark lands, then it is taken away at once', async () => {
    // Given a bridge that holds the mark call until the test lets it through, and a run that raised at line 4
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
    await pressRun();
    await waitFor(() => expect(status()).toBe('Wrong Answer'));

    // When the user types, so the card goes, and the held call is then let through
    await editor.type('x');
    await waitFor(cardIsGone);
    release();

    // Then the mark landed (the log says so) and does not stay: it was cleared as soon as it did
    await waitFor(async () => expect(await loggedText('editor_v2.run_error.shown')).toContain('"line":4'));
    await waitFor(async () => expect(await editor.marks()).toEqual([]));
    expect(editor.errorLines()).toEqual([]);
  });

  it('given a mark is shown, when the user resets the code, then the card and the mark are both gone', async () => {
    const r = routerHarness();
    const { editor } = await failedRun();

    await userEvent.click(screen.getByTestId('ProblemEditorResetButton'));
    await waitFor(() => expect(r.lastPush()).toBe('/leetcode/two-sum/reset-code'));
    await act(async () => useResetCodeRequestStore.getState().payload?.onConfirm());

    await waitFor(cardIsGone);
    await waitFor(async () => expect(await editor.marks()).toEqual([]));
    expect(editor.errorLines()).toEqual([]);
  });

  it('given a mark is shown, when the user switches the language, then the card and the mark are both gone', async () => {
    const { editor } = await failedRun();

    await userEvent.click(screen.getByTestId('ProblemEditorLanguagePicker-Trigger'));
    await waitFor(() => expect(screen.getByTestId('ProblemEditorLanguagePicker-JavaScript')).toBeTruthy());
    await userEvent.click(screen.getByTestId('ProblemEditorLanguagePicker-JavaScript'));

    await waitFor(cardIsGone);
    await waitFor(async () => expect(await editor.marks()).toEqual([]));
    expect(editor.errorLines()).toEqual([]);
  });
});

describe('the page goes where the line is', () => {
  const lineButton = () => screen.queryByTestId('ProblemEditorRunOutcomeLine');

  async function runWith(judge: FakePageScript['judge'], options: { editorBridge?: EditorBridgePort } = {}) {
    useRunnerSettingStore.setState({ runner: 'device' });
    const { runner } = deviceRunner({ judge });
    const session = await renderEditor({ deviceRunner: runner, ...options });
    const editor = await session.editor();
    await forgetLoggedLines();
    return { editor };
  }

  it('given a located error was shown, when the card lays out, then the page goes to the editor and the card is not scrolled to', async () => {
    const { editor } = await runWith((call) => answers.caseError(call, 'KeyError: 9', { line: 4 }, 1));

    await pressRun();

    await waitFor(async () => expect(await loggedText('editor_v2.run_error.scroll_to_editor')).toContain('"line":4'));
    expect(await loggedLines('editor_v2.run_outcome.reveal')).toEqual([]);
    expect(await markedTexts(editor)).toEqual([LINE_4]);
  });

  it('given a run without a line, when the card lays out, then the card is revealed as it always was and the page does not go to the editor', async () => {
    await runWith((call) => answers.wrongAnswer(call, 3));

    await pressRun();

    await waitFor(async () => expect((await loggedLines('editor_v2.run_outcome.reveal')).length).toBeGreaterThan(0));
    await waitFor(async () => expect(await loggedText('editor_v2.run_error.skipped')).toContain('no-location'));
    expect(await loggedLines('editor_v2.run_error.scroll_to_editor')).toEqual([]);
  });

  it('given the editor refused the mark, when the card lays out, then the card is revealed as the fallback and nothing is offered as Line N', async () => {
    const reportsAnotherPage = editorBridgeWith((real) => ({
      hello: async () => {
        const reply = await real.hello();
        return reply.ok ? { ...reply, value: { ...reply.value, instance: 'page-before-the-restart' } } : reply;
      },
    }));
    await runWith((call) => answers.caseError(call, 'KeyError: 9', { line: 4 }, 1), { editorBridge: reportsAnotherPage });

    await pressRun();

    await waitFor(async () => expect(await loggedText('editor_v2.run_error.refused')).toContain('instance-mismatch'));
    await waitFor(async () => expect((await loggedLines('editor_v2.run_outcome.reveal')).length).toBeGreaterThan(0));
    expect(await loggedLines('editor_v2.run_error.scroll_to_editor')).toEqual([]);
    expect(lineButton()).toBeNull();
  });

  it('given a located error and the card showing Line 4, when the user taps it, then the editor reveals line 4 again and the page goes to the editor', async () => {
    // Given the card of a run that raised at line 4, with the page scrolled down to the rest of the screen
    const { editor } = await runWith((call) => answers.caseError(call, 'KeyError: 9', { line: 4 }, 1));
    await pressRun();
    const button = await screen.findByTestId('ProblemEditorRunOutcomeLine');
    expect(button.textContent).toBe('Line 4');
    const page = screen.getByTestId('ProblemEditorScroll');
    page.scrollTop = 500;
    await forgetLoggedLines();

    // When the user taps Line 4
    await userEvent.click(button);

    // Then the editor was asked for the line again, it is revealed, and the page is back at the editor
    await waitFor(async () => expect(await loggedText('editor_v2.run_error.shown')).toContain('"line":4'));
    await waitFor(async () => expect(await loggedText('editor_v2.run_error.scroll_to_editor')).toContain('"line":4'));
    expect(await editor.revealedLine()).toBe(4);
    expect(page.scrollTop).toBe(0);
    expect(await markedTexts(editor)).toEqual([LINE_4]);
  });

  it('given a typed case that raised at line 4, when the learner presses Run input, then the custom card offers Line 4 too', async () => {
    await runWith((call) => answers.caseError(call, 'KeyError: 9', { line: 4 }));

    await pressRunInput();

    expect((await screen.findByTestId('ProblemEditorRunOutcomeLine')).textContent).toBe('Line 4');
    expect(status()).toBe('Error');
  });

  it('given a located error was shown, when the user types so the card goes, then no Line N is offered', async () => {
    const { editor } = await runWith((call) => answers.caseError(call, 'KeyError: 9', { line: 4 }, 1));
    await pressRun();
    await screen.findByTestId('ProblemEditorRunOutcomeLine');

    await editor.type('x');

    await waitFor(cardIsGone);
    expect(lineButton()).toBeNull();
  });

  it('given a run without a line, when the card is shown, then no Line N is offered', async () => {
    await runWith((call) => answers.wrongAnswer(call, 3));

    await pressRun();

    await waitFor(() => expect(status()).toBe('Wrong Answer'));
    await waitFor(async () => expect(await loggedText('editor_v2.run_error.skipped')).toContain('no-location'));
    expect(lineButton()).toBeNull();
  });
});

describe('a run that goes on while the person types', () => {
  // The held runner answers when the test says so. The text of the editor and of the screen are the same again once value syncs have run for each change.
  function heldRun() {
    let finish: (run: RunResponse) => void = () => undefined;
    const runner = createFakeCodeRunner(() => new Promise<RunResponse>((resolve) => (finish = resolve)));
    return { runner, finish: (run: RunResponse) => act(async () => finish(run)) };
  }

  it('given the run is going, when the user types a character and removes it and the run finishes at line 4, then the card is shown and the mark is on line 4', async () => {
    // Given a run held back on the six-line solution
    const { runner, finish } = heldRun();
    const session = await renderEditor({ runner });
    const editor = await session.editor();
    await pressRun();
    await waitFor(() => expect(runner.calls).toHaveLength(1));

    // When the user types a character and deletes it again (the revision moved by two, the text is equal), the screen having seen both, and the run then finishes
    await forgetLoggedLines();
    await editor.type('x');
    await waitFor(async () => expect(await valueSyncs()).toBeGreaterThanOrEqual(1));
    await editor.select(0, 1);
    await editor.type('');
    await waitFor(async () => expect(await valueSyncs()).toBeGreaterThanOrEqual(2));
    expect(editor.text()).toBe(SEED);
    await finish(FAILED_AT_LINE_4);

    // Then the card is shown and the mark follows the text across the two revisions to line 4
    await waitFor(() => expect(status()).toBe('Wrong Answer'));
    await waitFor(async () => expect(await markedTexts(editor)).toEqual([LINE_4]));
  });

  it('given the run is going, when the user types and the run finishes, then the verdict is dropped and nothing is marked', async () => {
    // Given a run held back
    const { runner, finish } = heldRun();
    const session = await renderEditor({ runner });
    const editor = await session.editor();
    await pressRun();
    await waitFor(() => expect(runner.calls).toHaveLength(1));

    // When the user types, the screen has seen it, and the run then finishes
    await forgetLoggedLines();
    await editor.type('x');
    await waitFor(async () => expect(await valueSyncs()).toBeGreaterThanOrEqual(1));
    await finish(FAILED_AT_LINE_4);

    // Then no card is shown for code that is no longer there and no line is marked
    await waitFor(async () => expect((await loggedLines('editor_v2.run_local.discarded')).length).toBeGreaterThan(0));
    cardIsGone();
    expect(await editor.marks()).toEqual([]);
    expect(editor.errorLines()).toEqual([]);
  });
});

describe('an editor that cannot give its text', () => {
  it('given an editor that never answers, when the user runs the tests, then the run goes ahead on the screen\'s text after the deadline, the card is shown and no line is marked', async () => {
    // Given a bridge whose hello is never answered, so no checkpoint can be taken
    useRunnerSettingStore.setState({ runner: 'device' });
    const { host, runner } = deviceRunner({ judge: (call) => answers.caseError(call, 'KeyError: 9', { line: 4 }, 1) });
    const session = await renderEditor({ deviceRunner: runner, editorBridge: editorBridgeWith(() => ({ hello: () => new Promise<never>(() => undefined) })) });
    const editor = await session.editor();
    await forgetLoggedLines();

    // When the user presses Run Local
    await pressRun();

    // Then the page judged the screen's text, the card shows the error, and the log says why there is no mark
    await waitFor(() => expect(screen.getByTestId('ProblemEditorRunOutcomeRuntimeError').textContent).toContain('KeyError: 9'), { timeout: 10_000 });
    const judged = host.calls.filter((call) => call.method === 'judge').map((call) => call.params as { payload: { code: string } });
    expect(judged[0]?.payload.code).toBe(SEED);
    expect(await loggedText('editor_v2.run_error.skipped')).toContain('no-checkpoint');
    expect(await editor.marks()).toEqual([]);
    expect(await editor.revealedLine()).toBeNull();
  });

  it('given the editor that was read at the run start is not the one that answers when the verdict arrives, when the run finishes, then nothing is marked and the log says refused', async () => {
    // Given a bridge that names the page as it was before a restart when the checkpoint is taken
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

    // When the user presses Run Local
    await pressRun();

    // Then the card is shown, the page refused the mark as another instance's, and nothing is drawn
    await waitFor(() => expect(screen.getByTestId('ProblemEditorRunOutcomeRuntimeError').textContent).toContain('KeyError: 9'));
    await waitFor(async () => expect(await loggedText('editor_v2.run_error.refused')).toContain('instance-mismatch'));
    expect(await editor.marks()).toEqual([]);
    expect(editor.errorLines()).toEqual([]);
  });
});
