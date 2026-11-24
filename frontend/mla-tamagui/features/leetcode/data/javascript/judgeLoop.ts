import type { Timers } from '@lib/webview-rpc';

import type { ErrorLocation, TestCase, TestResult } from '../../domain/run/execution';
import type { RawRun } from '../../domain/run/verdict';
import { JUDGE_DEFAULTS, type JudgePayload } from '../runner/judgeProtocol';
import { describeError, locationFromEvent, locationFromStack } from './harness/errorLocation';
import type { CaseOutcome, WorkerEngine } from './workerEngine';
import type { CaseRow } from './workerProtocol';
import { composeWorkerScript } from './workerSource';

export const OUTPUT_LIMIT = 4 * 1024 * 1024;

/** Whether the engine at hand reports a position the two ways the harness reads one: an ErrorEvent's line, and a stack's frames. Found by the page's boot self-test. */
export type LineSupport = { event: boolean; stack: boolean };

export type JudgeDeps = {
  engine: WorkerEngine;
  /** the harness, as the one line that goes in front of the learner's code */
  prelude: string;
  lines: LineSupport;
  /** the whole-run limit; the case deadlines are the engine's */
  timers: Timers;
};

const failed = (kind: 'compile' | 'runtime' | 'timeout', error: string, at?: ErrorLocation | null): RawRun => ({
  ok: false,
  results: [],
  error,
  error_kind: kind,
  ...(at ? { error_at: at } : {}),
});

/**
 * One run, as the Python judge's `_run`: the learner's code is loaded in a worker, each case runs in it, and every finished row is
 * handed to `emit` as it lands, so a worker that is ended still leaves the rows before it. Answers the raw run as JSON text and
 * leaves no worker behind.
 */
export async function runJudge(deps: JudgeDeps, payload: JudgePayload, emit: (row: string) => void): Promise<string> {
  try {
    const text = JSON.stringify(await execute(deps, payload, emit));
    return text.length > OUTPUT_LIMIT ? JSON.stringify(failed('runtime', 'Runner output exceeded the size limit')) : text;
  } catch (error) {
    return JSON.stringify(failed('runtime', `Runner error: ${describeError(error)}`));
  } finally {
    deps.engine.dispose();
  }
}

async function execute(deps: JudgeDeps, payload: JudgePayload, emit: (row: string) => void): Promise<RawRun> {
  const { engine, timers, lines } = deps;
  const caseSeconds = Math.max(1, Math.ceil((payload.timeoutMs ?? JUDGE_DEFAULTS.timeoutMs) / 1000));
  const caseMs = caseSeconds * 1000;
  const { script, firstLineOffset } = composeWorkerScript({
    prelude: deps.prelude,
    code: payload.code,
    config: {
      methodName: payload.methodName,
      paramTypes: payload.paramTypes ?? {},
      returnType: payload.returnType ?? null,
      inPlace: payload.inPlace ?? null,
      harness: payload.harness ?? null,
    },
  });

  const loaded = await engine.load(script, caseMs);
  if (loaded.kind === 'timeout') return failed('timeout', `Timeout: module-level code exceeded the ${caseSeconds}s limit`);
  if (loaded.kind === 'died') return failed('runtime', `Runner error: ${loaded.reason}`);
  if (loaded.kind === 'missing') return failed('compile', loaded.message);
  if (loaded.kind === 'failed') {
    const { message, line, column } = loaded.error;
    const at = lines.event ? locationFromEvent({ lineno: line, colno: column }, firstLineOffset) : null;
    return failed('compile', `CompileError: ${message}`, at);
  }

  // the whole-run limit is a timer of the host's, read between cases as the Python judge reads its clock
  let overLimit = false;
  const clock = timers.set(() => {
    overLimit = true;
  }, payload.processLimitMs ?? JUDGE_DEFAULTS.processLimitMs);
  const results: TestResult[] = [];
  try {
    for (const [index, testCase] of payload.testCases.entries()) {
      const outcome = await engine.runCase(index, testCase, caseMs);
      if (outcome.kind === 'died') return failed('runtime', `Runner error: ${outcome.reason}`);
      const result = rowOf(outcome, testCase, caseSeconds, lines, firstLineOffset);
      if (index === 0 && loaded.stdout !== '') result.stdout = loaded.stdout + result.stdout;
      results.push(result);
      announce(emit, JSON.stringify({ index, result }));
      if (overLimit) return failed('timeout', 'Timeout: execution exceeded process limit');
    }
  } finally {
    timers.clear(clock);
  }
  return { ok: true, results };
}

function announce(emit: (row: string) => void, row: string): void {
  try {
    emit(row);
    // oxlint-disable-next-line mobile-learn/no-unhandled-catch -- a failed progress event must not fail the run, as in the Python judge
  } catch {
    return;
  }
}

function errorRow(testCase: TestCase, error: string, elapsedMs: number, at: ErrorLocation | null): TestResult {
  return {
    pass: false,
    input: testCase.inputs,
    actual: null,
    expected: testCase.expected === undefined ? null : testCase.expected,
    stdout: '',
    stderr: '',
    error,
    elapsed_ms: elapsedMs,
    ...(at ? { error_at: at } : {}),
  };
}

function rowOf(outcome: Exclude<CaseOutcome, { kind: 'died' }>, testCase: TestCase, caseSeconds: number, lines: LineSupport, firstLineOffset: number): TestResult {
  // a timeout carries no error_at: the loop's own line is not where the learner's mistake is read
  if (outcome.kind === 'timeout') return errorRow(testCase, `TimeoutError: Code execution timed out (${caseSeconds}s limit)`, caseSeconds * 1000, null);
  if (outcome.kind === 'raised') {
    const { message, line, column } = outcome.error;
    return errorRow(testCase, message, 0, lines.event ? locationFromEvent({ lineno: line, colno: column }, firstLineOffset) : null);
  }
  return located(outcome.row, outcome.scriptName, lines.stack, firstLineOffset);
}

function located(row: CaseRow, scriptName: string, readStack: boolean, firstLineOffset: number): TestResult {
  const { stack, ...result } = row;
  const at = readStack && stack !== undefined ? locationFromStack(stack, scriptName, firstLineOffset) : null;
  return at ? { ...result, error_at: at } : result;
}
