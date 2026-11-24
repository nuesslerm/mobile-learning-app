import { describe, expect, it } from 'vitest';

import type { RunResponse, TestResult } from '../execution';
import { customOutcomeOf } from '../customOutcome';
import { runResponseOf, type RawRun } from '../verdict';

const INPUTS = { nums: [2, 7, 11, 15], target: 9 };

// what the harness answers for a typed case: one row, expected null, and a pass flag that compares nothing
const row = (over: Partial<TestResult> = {}): TestResult => ({ pass: false, input: INPUTS, actual: [0, 1], expected: null, stdout: '', stderr: '', error: null, elapsed_ms: 1.5, ...over });
const respond = (raw: RawRun, surfaceCaseError = true): RunResponse => runResponseOf(raw, 'offline', { surfaceCaseError });

describe('customOutcomeOf', () => {
  it('given a run whose single row has pass false and expected null but a value, when shaped, then the status is Custom case and the result is that value', () => {
    const outcome = customOutcomeOf(respond({ ok: true, results: [row()] }), INPUTS);
    expect(outcome).toMatchObject({ status: 'Custom case', hasResult: true, result: [0, 1], error: null, inputs: INPUTS });
    expect(outcome.errorAt).toBeUndefined();
  });

  it('given a response that says Accepted over a row that passed, when shaped, then the card is the same no-verdict card', () => {
    const outcome = customOutcomeOf(respond({ ok: true, results: [row({ pass: true, expected: [0, 1] })] }), INPUTS);
    expect(outcome).toMatchObject({ status: 'Custom case', hasResult: true, result: [0, 1] });
    expect(Object.keys(outcome)).not.toContain('pass');
    expect(Object.keys(outcome)).not.toContain('expected');
  });

  it('given a run whose row returned null, when shaped, then hasResult is true and the result is null', () => {
    expect(customOutcomeOf(respond({ ok: true, results: [row({ actual: null })] }), INPUTS)).toMatchObject({ hasResult: true, result: null, status: 'Custom case' });
  });

  it('given a case that raised, when shaped, then the status is Error, the error appears once though runtime_error repeats it, errorAt is kept and hasResult is false', () => {
    const response = respond({ ok: true, results: [row({ actual: null, error: 'IndexError: boom', error_at: { line: 4, column: 9 } })] });
    expect(response.runtime_error).toBe('IndexError: boom');
    expect(customOutcomeOf(response, INPUTS)).toEqual({
      status: 'Error',
      inputs: INPUTS,
      hasResult: false,
      result: null,
      error: 'IndexError: boom',
      errorAt: { line: 4, column: 9 },
      stdout: '',
      elapsedMs: 1.5,
    });
  });

  it('given a backend answer for a raising case, which lifts no error_at, when shaped, then it reads as an Error with no location', () => {
    const outcome = customOutcomeOf(respond({ ok: true, results: [row({ actual: null, error: 'IndexError: boom' })] }, false), INPUTS);
    expect(outcome).toMatchObject({ status: 'Error', error: 'IndexError: boom', hasResult: false });
    expect(outcome.errorAt).toBeUndefined();
  });

  it('given a compile error with no rows, when shaped, then the status is Error, the compile error is the text, its line is kept and the typed inputs are still echoed', () => {
    const response = respond({ ok: false, error: "CompileError: expected ':' (<string>, line 2)", error_kind: 'compile', error_at: { line: 2, column: 7 } });
    expect(customOutcomeOf(response, INPUTS)).toEqual({
      status: 'Error',
      inputs: INPUTS,
      hasResult: false,
      result: null,
      error: "CompileError: expected ':'",
      errorAt: { line: 2, column: 7 },
      stdout: '',
      elapsedMs: null,
    });
  });

  it("given the device runner's Timeout response with no rows, when shaped, then the status is Error with that text", () => {
    const response = respond({ ok: false, error: 'Timeout: execution exceeded process limit', error_kind: 'timeout' });
    expect(customOutcomeOf(response, INPUTS)).toMatchObject({ status: 'Error', error: 'Timeout: execution exceeded process limit', hasResult: false, inputs: INPUTS });
  });

  it('given a timeout that still carries the row collected before it, when shaped, then it is an Error and the row is not shown as a result', () => {
    const response = respond({ ok: false, error: 'Timeout: execution exceeded process limit', error_kind: 'timeout', results: [row()] });
    expect(customOutcomeOf(response, INPUTS)).toMatchObject({ status: 'Error', error: 'Timeout: execution exceeded process limit', hasResult: false, result: null });
  });

  it("given the response the screen builds when the runner itself fails, when shaped, then it reads as an Error with the exception's message", () => {
    const failed: RunResponse = { source: 'backend-run', kind: 'test', state: 'FAILURE', requestId: '', run_success: false, status_msg: 'Error', total_correct: 0, total_testcases: 0, runtime_error: 'Failed to fetch' };
    expect(customOutcomeOf(failed, INPUTS)).toMatchObject({ status: 'Error', error: 'Failed to fetch', hasResult: false, inputs: INPUTS, stdout: '', elapsedMs: null });
  });

  it('given a row with stdout and elapsed_ms, when shaped, then both are carried', () => {
    expect(customOutcomeOf(respond({ ok: true, results: [row({ stdout: 'hello\n', elapsed_ms: 12.25 })] }), INPUTS)).toMatchObject({ stdout: 'hello\n', elapsedMs: 12.25 });
  });
});
