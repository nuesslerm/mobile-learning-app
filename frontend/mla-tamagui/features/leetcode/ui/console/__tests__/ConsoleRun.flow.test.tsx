import { act, createFakeAudioRecorder, createFakeVoiceEditTransport, expectWarning, fireEvent, resetReducedMotion, screen, setReducedMotion, userEvent, waitFor, type FlowSession } from '@lib/flow-test-harness';
import { BackHandler, Platform } from 'react-native';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../../../../tests/flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../../tests/flow/loggedLines';
import type { CodeRunnerPort, LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../../..';
import { jsonText } from '../../../domain/console/caseFields';
import { CodeRunnerError } from '../../../domain/ports';
import { ProblemCatalog } from '../../../domain/problemCatalog';
import type { TestResult } from '../../../domain/run/execution';
import { createPyodideRunner, type RunnerLimits } from '../../../data/pyodide/pyodideRunner';
import { useConsoleCasesStore } from '../../../state/console/consoleCasesStore';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../../state/runnerSettingStore';
import { answers, createFakePageHost, createGatedCodeRunner, passingRun, type GatedCodeRunner } from '../../../testing';

const gateway = {
  userStatus: vi.fn(async () => ({ isSignedIn: true, username: 'flow-user' })),
  test: vi.fn(),
  submit: vi.fn(),
  poll: vi.fn(),
  runLocal: vi.fn(),
  format: vi.fn(async (p: { lang: string; code: string }) => p.code),
  loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })),
} as unknown as LeetCodeGatewayPort;

const parentheses = ProblemCatalog.getByIdOrThrow('valid-parentheses');
const twoSum = ProblemCatalog.getByIdOrThrow('two-sum');
const CASES = parentheses.testCases?.length ?? 0;

const SEED = ['class Solution:', '    def twoSum(self, nums: List[int], target: int) -> List[int]:', '        seen = {}', '        value = seen[target]', '        pairs = [value]', '        return pairs'].join('\n');
const LIMITS: Partial<RunnerLimits> = { readyMs: 2000, pullMs: 2000, bootMs: 2000, waitMs: 2000, runMs: 250, rpcSlackMs: 100 };

async function openEditor(options: { runner?: CodeRunnerPort; deviceRunner?: CodeRunnerPort; route?: string; seed?: string; flags?: { aiEdit: boolean } } = {}): Promise<FlowSession> {
  const ports: LeetCodePortOverrides = {
    gateway,
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
    ...(options.runner ? { runner: options.runner } : {}),
    ...(options.deviceRunner ? { deviceRunner: options.deviceRunner } : {}),
    ...(options.flags ? { flags: options.flags } : {}),
  };
  const session = await app.render(options.route ?? '/leetcode/valid-parentheses', { overrides: { leetcode: ports }, ...(options.seed ? { search: { seed: options.seed } } : {}) });
  await session.editor();
  return session;
}

const tap = (id: string) => userEvent.click(screen.getByTestId(id));
const text = (id: string) => screen.getByTestId(id).textContent;
const exists = (id: string) => screen.queryByTestId(id) !== null;
const isDisabled = (id: string) => screen.getByTestId(id).getAttribute('aria-disabled') === 'true' || (screen.getByTestId(id) as HTMLButtonElement).disabled;
const selected = (id: string) => screen.getByTestId(id).getAttribute('aria-selected');
const verdict = () => text('ConsoleResultVerdict');

async function setKeyboard(visible: boolean): Promise<void> {
  Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: visible });
  window.dispatchEvent(new Event('mcp:keyboardframe'));
  if (visible) await waitFor(() => expect(screen.queryByTestId('EditorHelperRails')).not.toBeNull(), { timeout: 5_000 });
}

function row(index: number, over: Partial<TestResult> = {}): TestResult {
  return { pass: true, input: { case: index }, actual: index, expected: index, stdout: '', stderr: '', error: null, elapsed_ms: 1, ...over };
}
function response(rows: TestResult[], over: Partial<RunResponse> = {}): RunResponse {
  const correct = rows.filter((entry) => entry.pass).length;
  return { source: 'offline', kind: 'test', state: correct === rows.length ? 'SUCCESS' : 'FAILURE', requestId: 'gated', run_success: true, status_msg: correct === rows.length ? 'Accepted' : 'Wrong Answer', total_correct: correct, total_testcases: rows.length, results: rows, ...over };
}
const rowsOf = (count: number) => Array.from({ length: count }, (_, index) => row(index));

function resetStores() {
  useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER });
  for (const problem of [parentheses, twoSum]) useConsoleCasesStore.getState().reset(problem);
  resetReducedMotion();
  Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: false });
}
beforeEach(resetStores);

describe('Run from the console', () => {
  it('given Case 3 selected and a gated runner, when Run is tapped on the closed console, then the runner holds that one bundled case, Run is disabled, and the released answer opens Run Result on Accepted', async () => {
    useConsoleCasesStore.getState().select(parentheses.id, 'bundled:2');
    const runner = createGatedCodeRunner();
    await openEditor({ runner });
    await forgetLoggedLines();

    await tap('ConsoleBarRun');

    await waitFor(() => expect(runner.held).toBe(1));
    expect(await loggedLines('editor_v2.console.run_start')).not.toHaveLength(0);
    expect(runner.calls[0]?.testCases).toEqual([parentheses.testCases?.[2]]);
    expect(isDisabled('ConsoleBarRun')).toBe(true);

    act(() => runner.release(passingRun(1)));

    await waitFor(() => expect(verdict()).toBe('Accepted'));
    expect(selected('ConsoleTabs-result')).toBe('true');
    expect(exists('ConsoleSheet')).toBe(true);
    expect(exists('ConsoleResultCounts')).toBe(false);
  });

  it('given a custom case with a field that is not JSON, when Run is tapped, then the runner is not called and the Testcase tab shows the message under the field', async () => {
    const runner = createGatedCodeRunner();
    const id = useConsoleCasesStore.getState().addCase(parentheses) ?? '';
    useConsoleCasesStore.getState().editField(parentheses.id, id, 's', "'(");
    await openEditor({ runner });

    await tap('ConsoleBarRun');

    await waitFor(() => expect(text('ConsoleFieldError')).toContain('s is not JSON'));
    expect(runner.calls).toHaveLength(0);
    expect(selected('ConsoleTabs-testcase')).toBe('true');
  });

  it('given a field focused in the sheet, when Run is tapped, then the field is blurred', async () => {
    const runner = createGatedCodeRunner();
    await openEditor({ runner });
    await tap('ConsoleBarOpen');
    await waitFor(() => expect(exists('ConsoleSheet')).toBe(true));
    await tap('ConsoleChipAdd');
    const field = screen.getByTestId('ConsoleField-s');
    act(() => field.focus());
    expect(document.activeElement).toBe(field);

    await tap('ConsoleFooterRun');

    await waitFor(() => expect(runner.held).toBe(1));
    expect(document.activeElement).not.toBe(field);
  });

  it('given a result on screen, when the code is typed over, then the result is gone', async () => {
    const runner = createGatedCodeRunner();
    const session = await openEditor({ runner });
    const editor = await session.editor();
    await tap('ConsoleBarRun');
    await waitFor(() => expect(runner.held).toBe(1));
    act(() => runner.release(passingRun(1)));
    await waitFor(() => expect(verdict()).toBe('Accepted'));

    await editor.type('x');

    await waitFor(() => expect(exists('ConsoleResultEmpty')).toBe(true));
    expect(exists('ConsoleResultVerdict')).toBe(false);
  });

  it('given the runner throws, when Run is tapped, then the tab says Run failed with the message and Run is enabled again', async () => {
    expectWarning(/editor_v2\.run_local\.error/);
    const runner = createGatedCodeRunner();
    await openEditor({ runner });
    await tap('ConsoleBarRun');
    await waitFor(() => expect(runner.held).toBe(1));

    act(() => runner.release(new CodeRunnerError('unavailable', 'The runner is not available')));

    await waitFor(() => expect(verdict()).toBe('Run failed'));
    expect(text('ConsoleResultErrorText')).toBe('The runner is not available');
    expect(await loggedLines('editor_v2.run_local.error')).not.toHaveLength(0);
    await tap('ConsoleTabs-testcase');
    expect(isDisabled('ConsoleFooterRun')).toBe(false);
  });
});

describe('the pending overlay', () => {
  it('given a gated run, when Run is tapped, then the overlay shows as a busy progressbar until the answer lands', async () => {
    const runner = createGatedCodeRunner();
    await openEditor({ runner });

    await tap('ConsoleBarRun');

    await waitFor(() => expect(exists('ConsolePending')).toBe(true));
    expect(screen.getByTestId('ConsolePending').getAttribute('role')).toBe('progressbar');
    expect(screen.getByTestId('ConsolePending').getAttribute('aria-busy')).toBe('true');
    expect(text('ConsolePendingText')).toBe('Pending...');
    expect(exists('ConsoleScan')).toBe(true);

    act(() => runner.release(passingRun(1)));

    await waitFor(() => expect(exists('ConsolePending')).toBe(false));
  });

  it('given a pending run and no motion preference, when the glyph shows, then the scan line is at its animated start; with reduced motion it is frozen at 60%', async () => {
    const runner = createGatedCodeRunner();
    await openEditor({ runner });
    await tap('ConsoleBarRun');
    await waitFor(() => expect(exists('ConsoleScan')).toBe(true));
    expect(screen.getByTestId('ConsoleScan').getAttribute('style')).toContain('translateY(0px)');
    act(() => runner.release(passingRun(1)));
    await waitFor(() => expect(exists('ConsolePending')).toBe(false));

    act(() => setReducedMotion(true));
    await tap('ConsoleFooterRun');
    await waitFor(() => expect(exists('ConsoleScan')).toBe(true));

    expect(screen.getByTestId('ConsoleScan').getAttribute('style')).toContain('translateY(36px)');
    act(() => runner.release(passingRun(1)));
    await waitFor(() => expect(exists('ConsolePending')).toBe(false));
  });
});

describe('the result cards', () => {
  const runAndRelease = async (runner: GatedCodeRunner, answer: RunResponse, button = 'ConsoleBarRun') => {
    await tap(button);
    await waitFor(() => expect(runner.held).toBe(1));
    act(() => runner.release(answer));
    await waitFor(() => expect(exists('ConsoleResultVerdict')).toBe(true));
  };

  it('given a bundled case that passes, when it runs, then Input, Output and Expected show as JSON with the runtime', async () => {
    const runner = createGatedCodeRunner();
    await openEditor({ runner });

    await runAndRelease(runner, response([row(0, { input: { s: '()' }, actual: true, expected: true, elapsed_ms: 2.4 })]));

    expect(text('ConsoleResultInput-s')).toBe('"()"');
    expect(text('ConsoleResultOutput')).toBe('true');
    expect(text('ConsoleResultExpected')).toBe('true');
    expect(text('ConsoleResultRuntime')).toBe('Runtime 2 ms');
  });

  it('given a wrong answer with stdout, when it runs, then the verdict, the red Output, Expected and the stdout card show', async () => {
    const runner = createGatedCodeRunner();
    await openEditor({ runner });

    await runAndRelease(runner, response([row(0, { pass: false, input: { s: '(]' }, actual: true, expected: false, stdout: 'saw it\n' })]));

    expect(verdict()).toBe('Wrong Answer');
    expect(text('ConsoleResultOutput')).toBe('true');
    expect(text('ConsoleResultExpected')).toBe('false');
    expect(text('ConsoleResultStdout')).toContain('saw it');
  });

  it('given a custom case, when it runs, then the headline is Custom case and there is no expected value', async () => {
    const runner = createGatedCodeRunner();
    await openEditor({ runner });
    await tap('ConsoleBarOpen');
    await waitFor(() => expect(exists('ConsoleSheet')).toBe(true));
    await tap('ConsoleChipAdd');

    await runAndRelease(runner, response([row(0, { pass: false, input: { s: '()' }, actual: true, expected: null })]), 'ConsoleFooterRun');

    expect(verdict()).toBe('Custom case');
    expect(exists('ConsoleResultExpected')).toBe(false);
    expect(text('ConsoleResultOutput')).toBe('true');
    expect(runner.calls[0]?.testCases[0]?.expected).toBeNull();
  });

  it('given a case that raises at line 4, when it runs, then Runtime Error shows the text, the line, and the input that ran, and Open Testcase selects its chip', async () => {
    useConsoleCasesStore.getState().select(parentheses.id, 'bundled:2');
    const runner = createGatedCodeRunner();
    await openEditor({ runner });

    await runAndRelease(runner, response([row(0, { pass: false, input: { s: '([)]' }, error: 'IndentationError: expected an indented block', error_at: { line: 4 }, actual: null })], { runtime_error: 'IndentationError: expected an indented block' }));

    expect(verdict()).toBe('Runtime Error');
    expect(text('ConsoleResultErrorText')).toContain('IndentationError');
    expect(text('ConsoleResultLine')).toBe('Line 4 (Solution.py)');
    expect(text('ConsoleResultLast-s-Card')).toContain('s =');
    expect(text('ConsoleResultLast-s')).toBe('"([)]"');

    await tap('ConsoleResultOpenCase');

    expect(selected('ConsoleTabs-testcase')).toBe('true');
    expect(selected('ConsoleChip-3')).toBe('true');
  });

  it('given a case deleted after the run, when the result is read, then Open Testcase is absent', async () => {
    const runner = createGatedCodeRunner();
    await openEditor({ runner });
    await tap('ConsoleBarOpen');
    await waitFor(() => expect(exists('ConsoleSheet')).toBe(true));
    await tap('ConsoleChipAdd');
    await runAndRelease(runner, response([row(0, { pass: false, input: { s: '()' }, error: 'ValueError: x', actual: null })]), 'ConsoleFooterRun');
    expect(exists('ConsoleResultOpenCase')).toBe(true);

    await tap('ConsoleTabs-testcase');
    await tap('ConsoleDeleteCase');
    await tap('ConsoleTabs-result');

    expect(exists('ConsoleResultOpenCase')).toBe(false);
  });

  it('given a hung case, when it runs, then Time Limit Exceeded shows', async () => {
    const runner = createGatedCodeRunner();
    await openEditor({ runner });

    await runAndRelease(runner, response([row(0, { pass: false, error: 'TimeoutError: Code execution timed out (5s limit)', actual: null })]));

    expect(verdict()).toBe('Time Limit Exceeded');
  });

  it('given a syntax error, when it runs, then Compile Error shows its line and no Last Executed Input', async () => {
    const runner = createGatedCodeRunner();
    await openEditor({ runner });

    await runAndRelease(runner, response([], { run_success: false, status_msg: 'Compile Error', compile_error: "SyntaxError: expected ':'", error_at: { line: 2 } }));

    expect(verdict()).toBe('Compile Error');
    expect(text('ConsoleResultLine')).toBe('Line 2 (Solution.py)');
    expect(exists('ConsoleResultLastInput')).toBe(false);
  });
});

describe('the error line', () => {
  it('given the device runner on a fake page that raises at line 4, when Run is tapped, then the editor marks line 4, the result shows it, and tapping it closes the sheet and reveals the line', async () => {
    useRunnerSettingStore.setState({ runner: 'device' });
    const host = createFakePageHost({ readyAfterMs: 5, judge: (call) => answers.caseError(call, 'KeyError: 9', { line: 4 }) });
    const session = await openEditor({ route: '/leetcode/two-sum', seed: SEED, deviceRunner: createPyodideRunner({ host, limits: LIMITS }) });
    const editor = await session.editor();

    await tap('ConsoleBarRun');

    await waitFor(() => expect(verdict()).toBe('Runtime Error'));
    await waitFor(async () => expect((await editor.marks()).length).toBe(1));
    await waitFor(() => expect(editor.errorLines().join('')).toBe('value = seen[target]'));
    expect(text('ConsoleResultLine')).toBe('Line 4 (Solution.py)');
    expect(editor.focused()).toBe(false);

    await waitFor(() => expect(screen.getByTestId('ConsoleResultLine').tagName).toBe('BUTTON'));
    await tap('ConsoleResultLine');

    await waitFor(() => expect(exists('ConsoleSheet')).toBe(false));
    expect(await editor.revealedLine()).toBe(4);

    await editor.type('x');
    await waitFor(async () => expect(await editor.marks()).toEqual([]));
  });
});

describe('Run from the rail', () => {
  it('given the editor focused and the rail showing the bar, when Run is tapped, then the keyboard is dismissed before the run starts and the sheet opens on Run Result', async () => {
    const runner = createGatedCodeRunner();
    await openEditor({ runner });
    await setKeyboard(true);
    await forgetLoggedLines();

    await tap('ConsoleBarRun');

    await waitFor(() => expect(runner.held).toBe(1));
    const lines = (await loggedLines()).map((entry) => entry.text);
    const dismissed = lines.findIndex((line) => line.includes('editor_v2.keyboard_dismiss'));
    const started = lines.findIndex((line) => line.includes('editor_v2.console.run_start'));
    expect(dismissed).toBeGreaterThanOrEqual(0);
    expect(dismissed).toBeLessThan(started);
    act(() => runner.release(passingRun(1)));
    await waitFor(() => expect(verdict()).toBe('Accepted'));
    expect(exists('ConsoleSheet')).toBe(true);
  });
});

describe('a stale answer and the locks while pending', () => {
  it('given a gated run, when the code is typed over before the release, then the answer is dropped, the tab does not switch and the overlay ends', async () => {
    const runner = createGatedCodeRunner();
    const session = await openEditor({ runner });
    const editor = await session.editor();
    await tap('ConsoleBarRun');
    await waitFor(() => expect(runner.held).toBe(1));
    await forgetLoggedLines();

    await editor.type('x');
    await waitFor(async () => expect(await loggedLines('editor.value_sync')).not.toHaveLength(0));
    act(() => runner.release(passingRun(1)));

    await waitFor(() => expect(exists('ConsolePending')).toBe(false));
    await waitFor(async () => expect(await loggedLines('editor_v2.run_local.discarded')).not.toHaveLength(0));
    expect(exists('ConsoleResultVerdict')).toBe(false);
    expect(exists('ConsoleSheet')).toBe(false);
  });

  it('given the console open and a run pending, when the Plan tab is tapped, then the sheet stays open', async () => {
    const runner = createGatedCodeRunner();
    await openEditor({ runner, flags: { aiEdit: true } });
    await tap('ConsoleBarOpen');
    await waitFor(() => expect(exists('ConsoleSheet')).toBe(true));
    await tap('ConsoleFooterRun');
    await waitFor(() => expect(runner.held).toBe(1));

    await tap('ProblemWorkspaceTab-plan');

    expect(exists('ConsoleSheet')).toBe(true);
    act(() => runner.release(passingRun(1)));
    await waitFor(() => expect(exists('ConsolePending')).toBe(false));
  });

  describe('on Android', () => {
    const realOS = Platform.OS;
    afterEach(() => {
      (Platform as { OS: string }).OS = realOS;
      vi.restoreAllMocks();
    });

    it('given a run pending with the console open, when the back handler runs, then it answers true and does not close', async () => {
      (Platform as { OS: string }).OS = 'android';
      const handlers: Parameters<typeof BackHandler.addEventListener>[1][] = [];
      // oxlint-disable-next-line no-restricted-properties -- react-native's BackHandler is a native API with no runtime in jsdom
      vi.spyOn(BackHandler, 'addEventListener').mockImplementation((_event, handler) => {
        handlers.push(handler);
        return { remove: () => void handlers.splice(handlers.indexOf(handler), 1) };
      });
      const runner = createGatedCodeRunner();
      await openEditor({ runner });
      await tap('ConsoleBarOpen');
      await waitFor(() => expect(handlers).toHaveLength(1));
      await tap('ConsoleFooterRun');
      await waitFor(() => expect(runner.held).toBe(1));

      let answer = false;
      act(() => {
        answer = handlers[0]?.({} as never) ?? false;
      });

      expect(answer).toBe(true);
      expect(exists('ConsoleSheet')).toBe(true);
      act(() => runner.release(passingRun(1)));
      await waitFor(() => expect(exists('ConsolePending')).toBe(false));
    });
  });
});

describe('Submit', () => {
  it('given two custom chips and Case 3 selected, when Submit is tapped, then the runner holds the bundled cases in catalog order and the passing answer reads Accepted with n / n and no IO cards', async () => {
    const runner = createGatedCodeRunner();
    act(() => {
      useConsoleCasesStore.getState().addCase(parentheses);
      useConsoleCasesStore.getState().addCase(parentheses);
      useConsoleCasesStore.getState().select(parentheses.id, 'bundled:2');
    });
    await openEditor({ runner });
    await tap('ConsoleBarOpen');
    await waitFor(() => expect(exists('ConsoleSheet')).toBe(true));

    await tap('ConsoleFooterSubmit');

    await waitFor(() => expect(runner.held).toBe(1));
    expect(runner.calls[0]?.testCases).toEqual(parentheses.testCases);
    expect(exists('ConsolePending')).toBe(true);
    act(() => runner.release(response(rowsOf(CASES))));

    await waitFor(() => expect(verdict()).toBe('Accepted'));
    expect(text('ConsoleResultCounts')).toBe(`${CASES} / ${CASES} testcases passed`);
    expect(text('ConsoleResultRuntime')).toMatch(/^Runtime \d+ ms$/);
    expect(exists('ConsoleResultOutput')).toBe(false);
  });

  it('given case 2 and case 9 fail, when the answer lands, then Wrong Answer shows k / m and case 2, and Open Testcase selects Case 2', async () => {
    const runner = createGatedCodeRunner();
    await openEditor({ runner });
    const rows = rowsOf(CASES);
    rows[1] = row(1, { pass: false, input: { s: 'two' }, actual: 'bad2', expected: 'ok2' });
    rows[8] = row(8, { pass: false, input: { s: 'nine' }, actual: 'bad9', expected: 'ok9' });

    await tap('ConsoleBarSubmit');
    await waitFor(() => expect(runner.held).toBe(1));
    act(() => runner.release(response(rows)));

    await waitFor(() => expect(verdict()).toBe('Wrong Answer'));
    expect(text('ConsoleResultCounts')).toBe(`${CASES - 2} / ${CASES} testcases passed`);
    expect(text('ConsoleResultInput-s')).toBe(jsonText('two'));
    expect(text('ConsoleResultOutput')).toBe(jsonText('bad2'));
    expect(text('ConsoleResultExpected')).toBe(jsonText('ok2'));
  });

  it('given case 4 raises at line 3, when the answer lands, then Runtime Error shows the line, the counts and case 4 as the last input', async () => {
    const runner = createGatedCodeRunner();
    await openEditor({ runner });
    const rows = rowsOf(CASES);
    rows[3] = row(3, { pass: false, input: { s: 'four' }, error: 'ZeroDivisionError: division by zero', error_at: { line: 3 }, actual: null });

    await tap('ConsoleBarSubmit');
    await waitFor(() => expect(runner.held).toBe(1));
    act(() => runner.release(response(rows)));

    await waitFor(() => expect(verdict()).toBe('Runtime Error'));
    expect(text('ConsoleResultLine')).toBe('Line 3 (Solution.py)');
    expect(text('ConsoleResultCounts')).toBe(`${CASES - 1} / ${CASES} testcases passed`);
    expect(text('ConsoleResultLast-s')).toBe(jsonText('four'));
    await tap('ConsoleResultOpenCase');
    expect(selected('ConsoleChip-4')).toBe('true');
  });

  it('given a process-limit timeout after 6 rows, when the answer lands, then Time Limit Exceeded names the seventh case', async () => {
    const runner = createGatedCodeRunner();
    await openEditor({ runner });

    await tap('ConsoleBarSubmit');
    await waitFor(() => expect(runner.held).toBe(1));
    act(() => runner.release(response(rowsOf(6), { run_success: false, status_msg: 'Time Limit Exceeded', runtime_error: 'Timeout: execution exceeded process limit', total_testcases: CASES })));

    await waitFor(() => expect(verdict()).toBe('Time Limit Exceeded'));
    await tap('ConsoleResultOpenCase');
    expect(selected('ConsoleChip-7')).toBe('true');
  });

  it('given a compile error, when the answer lands, then Compile Error shows with its line and no input', async () => {
    const runner = createGatedCodeRunner();
    await openEditor({ runner });

    await tap('ConsoleBarSubmit');
    await waitFor(() => expect(runner.held).toBe(1));
    act(() => runner.release(response([], { run_success: false, status_msg: 'Compile Error', compile_error: 'SyntaxError: invalid syntax', error_at: { line: 2 } })));

    await waitFor(() => expect(verdict()).toBe('Compile Error'));
    expect(text('ConsoleResultLine')).toBe('Line 2 (Solution.py)');
    expect(exists('ConsoleResultLastInput')).toBe(false);
  });

  it('given a text field in the sheet focused, when a bad custom case is selected and Submit is tapped, then Submit ignores the custom chip and runs the bundled set', async () => {
    const runner = createGatedCodeRunner();
    const id = useConsoleCasesStore.getState().addCase(parentheses) ?? '';
    useConsoleCasesStore.getState().editField(parentheses.id, id, 's', "'(");
    await openEditor({ runner });

    await tap('ConsoleBarSubmit');

    await waitFor(() => expect(runner.held).toBe(1));
    expect(runner.calls[0]?.testCases).toEqual(parentheses.testCases);
    fireEvent.click(document.body);
    act(() => runner.release(response(rowsOf(CASES))));
    await waitFor(() => expect(verdict()).toBe('Accepted'));
  });
});
