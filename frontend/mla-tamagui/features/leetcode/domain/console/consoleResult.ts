import { isJavaScriptLanguage, isPythonLanguage } from '../run/runLanguage';
import type { ErrorLocation, RunResponse, TestResult } from '../run/execution';
import { customOutcomeOf } from '../run/customOutcome';
import type { RunPlan } from './runPlan';

export type CaseFocus = { label: string; inputs: Record<string, unknown>; actual: unknown; expected: unknown; stdout: string };
export type LastInput = { label: string; inputs: Record<string, unknown> };
export type ResultCounts = { correct: number; total: number };

type Shared = { mode: RunPlan['mode']; runtimeMs: number; counts: ResultCounts | null; stdout: string };
type Failure = Shared & { message: string; errorAt?: ErrorLocation; fileName: string; lastInput: LastInput | null };

export type ConsoleResult =
  | (Shared & { kind: 'accepted'; focus: CaseFocus | null })
  | (Shared & { kind: 'wrong-answer'; focus: CaseFocus })
  | (Failure & { kind: 'runtime-error' })
  | (Failure & { kind: 'time-limit' })
  | (Failure & { kind: 'compile-error' })
  | (Shared & { kind: 'custom'; inputs: Record<string, unknown>; result: unknown })
  | { kind: 'run-failed'; message: string };

export type ConsoleVerdict = ConsoleResult['kind'];

const TIMEOUT_PREFIXES = ['TimeoutError: Code execution timed out', 'Timeout:'];
const MODULE_LEVEL_TIMEOUT = 'Timeout: module-level';

/** The judge's own words for a run or a case that ran out of time; pinned against the real judge in `consoleVerdicts.test.ts`. */
export const isTimeoutText = (text: string): boolean => TIMEOUT_PREFIXES.some((prefix) => text.startsWith(prefix));

export function fileNameOf(lang: string): string {
  if (isPythonLanguage(lang)) return 'Solution.py';
  if (isJavaScriptLanguage(lang)) return 'Solution.js';
  return 'Solution.cpp';
}

const stdoutOf = (rows: readonly TestResult[]) => rows.map((row) => row.stdout).join('');

export function consoleResultOf(plan: RunPlan, response: RunResponse): ConsoleResult {
  const rows = response.results ?? [];
  const shared: Shared = {
    mode: plan.mode,
    runtimeMs: rows.reduce((sum, row) => sum + row.elapsed_ms, 0),
    counts: plan.mode === 'submit' ? { correct: response.total_correct, total: response.total_testcases } : null,
    stdout: stdoutOf(rows),
  };
  const fileName = fileNameOf(plan.lang);
  const failure = (message: string, errorAt: ErrorLocation | undefined, lastInput: LastInput | null): Failure => ({ ...shared, message, ...(errorAt ? { errorAt } : {}), fileName, lastInput });
  const errorResult = (message: string, errorAt: ErrorLocation | undefined, lastInput: LastInput | null): ConsoleResult =>
    isTimeoutText(message) ? { kind: 'time-limit', ...failure(message, undefined, lastInput) } : { kind: 'runtime-error', ...failure(message, errorAt, lastInput) };
  const lastInputAt = (index: number): LastInput | null => {
    const planned = plan.cases[index];
    return planned ? { label: planned.label, inputs: rows[index]?.input ?? planned.inputs } : null;
  };

  if (response.compile_error) return { kind: 'compile-error', ...failure(response.compile_error, response.error_at, null) };

  if (plan.custom) {
    const outcome = customOutcomeOf(response, plan.cases[0]?.inputs ?? {});
    if (outcome.error === null) return { kind: 'custom', ...shared, inputs: outcome.inputs, result: outcome.result };
    return errorResult(outcome.error, outcome.errorAt, lastInputAt(0));
  }

  const failedAt = rows.findIndex((row) => !row.pass);
  const failed = failedAt >= 0 ? rows[failedAt] : undefined;
  if (failed) {
    if (failed.error !== null) return errorResult(failed.error, failed.error_at, lastInputAt(failedAt));
    const input = lastInputAt(failedAt);
    return { kind: 'wrong-answer', ...shared, focus: { label: input?.label ?? `Case ${failedAt + 1}`, inputs: input?.inputs ?? {}, actual: failed.actual, expected: failed.expected, stdout: failed.stdout } };
  }
  if (response.runtime_error !== undefined || !response.run_success) {
    const message = response.runtime_error ?? 'Unknown error';
    if (isTimeoutText(message)) return { kind: 'time-limit', ...failure(message, undefined, message.startsWith(MODULE_LEVEL_TIMEOUT) ? null : lastInputAt(rows.length)) };
    return { kind: 'runtime-error', ...failure(message, response.error_at, null) };
  }
  const only = plan.mode === 'run' ? rows[0] : undefined;
  const input = lastInputAt(0);
  return {
    kind: 'accepted',
    ...shared,
    focus: only && input ? { label: input.label, inputs: input.inputs, actual: only.actual, expected: only.expected, stdout: only.stdout } : null,
  };
}

/** A runner that could not run at all is not a verdict: the screen says the run failed. */
export function consoleResultOfFailure(error: unknown): ConsoleResult {
  return { kind: 'run-failed', message: error instanceof Error ? error.message : String(error) };
}
