import { describe, expect, it } from 'vitest';

import type { TestResult } from '../execution';
import { runResponseOf } from '../verdict';

const row = (over: Partial<TestResult> = {}): TestResult => ({ pass: true, actual: 1, expected: 1, stdout: '', stderr: '', error: null, elapsed_ms: 1, ...over });

describe('runResponseOf', () => {
  it('given every case passed, when the run is shaped, then it is Accepted with the totals', () => {
    const response = runResponseOf({ ok: true, results: [row(), row()] }, 'offline');
    expect(response).toMatchObject({ source: 'offline', kind: 'test', state: 'SUCCESS', run_success: true, status_msg: 'Accepted', total_correct: 2, total_testcases: 2 });
  });

  it('given a case failed, when the run is shaped, then it is Wrong Answer and names no error', () => {
    const response = runResponseOf({ ok: true, results: [row(), row({ pass: false, actual: 2 })] }, 'backend-run');
    expect(response).toMatchObject({ state: 'FAILURE', run_success: true, status_msg: 'Wrong Answer', total_correct: 1, total_testcases: 2 });
    expect(response.runtime_error).toBeUndefined();
  });

  it('given no cases at all, when the run is shaped, then it stays Accepted 0 of 0 as the backend answers it', () => {
    expect(runResponseOf({ ok: true, results: [] }, 'backend-run')).toMatchObject({ status_msg: 'Accepted', total_correct: 0, total_testcases: 0 });
  });

  it.each([
    ['a compile error with no kind', { ok: false, error: "CompileError: expected ':'" }, { compile_error: "CompileError: expected ':'" }],
    ['a timeout with no kind', { ok: false, error: 'Timeout: execution exceeded process limit' }, { runtime_error: 'Timeout: execution exceeded process limit' }],
    ['a runtime kind', { ok: false, error: 'Runner error: X', error_kind: 'runtime' as const }, { runtime_error: 'Runner error: X' }],
  ])('given %s, when the run is shaped, then the error lands in the right field', (_name, raw, fields) => {
    const response = runResponseOf(raw, 'backend-run');
    expect(response).toMatchObject({ state: 'FAILURE', run_success: false, status_msg: 'Error', ...fields });
    expect(response.compile_error === undefined).toBe(!('compile_error' in fields));
  });

  it('given a compile error with its line, when the run is shaped, then the line moves to error_at and leaves the text', () => {
    const response = runResponseOf({ ok: false, error: "CompileError: expected ':' (<string>, line 2)", error_kind: 'compile', error_at: { line: 2, column: 7 } }, 'offline');
    expect(response).toMatchObject({ compile_error: "CompileError: expected ':'", error_at: { line: 2, column: 7 } });
  });

  it('given a backend compile error whose text ends with the line Python printed and no error_at, when the run is shaped, then the line is read from the text and leaves it', () => {
    const response = runResponseOf({ ok: false, error: 'CompileError: invalid syntax (<string>, line 2)' }, 'backend-run');
    expect(response).toMatchObject({ compile_error: 'CompileError: invalid syntax', error_at: { line: 2 } });
    expect(response.error_at).toEqual({ line: 2 });
  });

  it('given the same text with an error_at from the device, when the run is shaped, then the device location is kept and the text is as for any located compile error', () => {
    const response = runResponseOf({ ok: false, error: 'CompileError: invalid syntax (<string>, line 2)', error_kind: 'compile', error_at: { line: 5, column: 3 } }, 'offline');
    expect(response).toMatchObject({ compile_error: 'CompileError: invalid syntax', error_at: { line: 5, column: 3 } });
  });

  it.each([
    ['no line in the text', { ok: false, error: "CompileError: expected ':'" }],
    ['a JavaScript error that mentions a line elsewhere in its text', { ok: false, error: 'CompileError: Unexpected token at line 3 of the snippet', error_kind: 'compile' as const }],
    ['a line that is not the end of the text', { ok: false, error: 'CompileError: invalid syntax (<string>, line 2) in Solution' }],
    ['another file name', { ok: false, error: 'CompileError: invalid syntax (solution.py, line 2)' }],
  ])('given a backend compile error with %s, when the run is shaped, then no location is derived and the text is untouched', (_name, raw) => {
    const response = runResponseOf(raw, 'backend-run');
    expect(response.error_at).toBeUndefined();
    expect(response.compile_error).toBe(raw.error);
  });

  it.each([
    ['a runtime kind', { ok: false, error: 'Runner error: invalid syntax (<string>, line 2)', error_kind: 'runtime' as const }],
    ['a timeout with no kind', { ok: false, error: 'Timeout: execution exceeded process limit (<string>, line 2)' }],
  ])('given %s whose text ends with a printed line, when the run is shaped, then no location is derived and the text is untouched', (_name, raw) => {
    const response = runResponseOf(raw, 'backend-run');
    expect(response.error_at).toBeUndefined();
    expect(response.compile_error).toBeUndefined();
    expect(response.runtime_error).toBe(raw.error);
  });

  it('given a total the harness reports, when the run is shaped, then it counts that many cases, not the rows it got', () => {
    expect(runResponseOf({ ok: false, error: 'Timeout: execution exceeded process limit', total: 12, results: [row()] }, 'offline')).toMatchObject({ total_testcases: 12, total_correct: 1 });
  });

  it('given a raising case and surfaceCaseError, when the run is shaped, then its error and line are on the response and the verdict stays Wrong Answer', () => {
    const raw = { ok: true, results: [row(), row({ pass: false, actual: null, error: 'ZeroDivisionError: division by zero', error_at: { line: 3 } })] };
    expect(runResponseOf(raw, 'offline', { surfaceCaseError: true })).toMatchObject({ status_msg: 'Wrong Answer', runtime_error: 'ZeroDivisionError: division by zero', error_at: { line: 3 } });
    expect(runResponseOf(raw, 'backend-run').runtime_error).toBeUndefined();
  });
});
