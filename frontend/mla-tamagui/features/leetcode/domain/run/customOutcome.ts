import type { ErrorLocation, RunResponse } from './execution';

export type CustomOutcome = {
  status: 'Custom case' | 'Error';
  /** What the learner typed: a compile error, a timeout or a runner failure returns no row to read it from. */
  inputs: Record<string, unknown>;
  /** False when the case raised or the run failed: `result` is then meaningless and the card draws a dash. */
  hasResult: boolean;
  result: unknown;
  error: string | null;
  errorAt?: ErrorLocation;
  stdout: string;
  elapsedMs: number | null;
};

/**
 * One typed case as a card with no verdict: `pass`, `expected`, `status_msg` and the totals describe a comparison that did not happen,
 * so none is read. The error shows once: `runtime_error` repeats the first raising case's error when the runner surfaces it.
 */
export function customOutcomeOf(response: RunResponse, inputs: Record<string, unknown>): CustomOutcome {
  const row = response.results?.[0];
  const error = row?.error ?? response.compile_error ?? response.runtime_error ?? null;
  const errorAt = row?.error_at ?? response.error_at;
  const hasResult = row !== undefined && error === null;
  return {
    status: error === null ? 'Custom case' : 'Error',
    inputs,
    hasResult,
    result: hasResult ? row.actual : null,
    error,
    ...(errorAt ? { errorAt } : {}),
    stdout: row?.stdout ?? '',
    elapsedMs: row?.elapsed_ms ?? null,
  };
}
