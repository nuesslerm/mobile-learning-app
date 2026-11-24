import { describe, expect, it } from 'vitest';

import { CodeRunnerError } from '../../ports';
import type { RunResponse, TestResult } from '../../run/execution';
import { runResponseOf } from '../../run/verdict';
import { consoleResultOf, consoleResultOfFailure, isTimeoutText } from '../consoleResult';
import type { PlannedCase, RunPlan } from '../runPlan';

const cases = (n: number): PlannedCase[] => Array.from({ length: n }, (_, i) => ({ id: `bundled:${i}`, label: `Case ${i + 1}`, inputs: { s: `in${i}` } }));
const planOf = (n: number, over: Partial<RunPlan> = {}): RunPlan => ({
  mode: 'submit',
  lang: 'python3',
  params: { lang: 'python3', code: '', methodName: 'f', testCases: [] },
  cases: cases(n),
  custom: false,
  ...over,
});
const row = (over: Partial<TestResult> = {}): TestResult => ({ pass: true, actual: 1, expected: 1, stdout: '', stderr: '', error: null, elapsed_ms: 2, ...over });
const respond = (rows: TestResult[], extra: Parameters<typeof runResponseOf>[0] = {}) =>
  runResponseOf({ ok: true, results: rows, ...extra }, 'offline', { surfaceCaseError: true });

describe('consoleResultOf', () => {
  it('given a Submit whose 15 cases pass, when read, then it is accepted with 15/15 and the summed runtime', () => {
    const result = consoleResultOf(planOf(15), respond(Array.from({ length: 15 }, () => row())));
    expect(result).toMatchObject({ kind: 'accepted', counts: { correct: 15, total: 15 }, runtimeMs: 30, focus: null });
  });

  it('given case 2 and case 9 fail, when read, then it is wrong-answer 13/15 focused on case 2', () => {
    const rows = Array.from({ length: 15 }, (_, i) => (i === 1 || i === 8 ? row({ pass: false, actual: 'a', expected: 'b' }) : row()));
    const result = consoleResultOf(planOf(15), respond(rows));
    expect(result).toMatchObject({ kind: 'wrong-answer', counts: { correct: 13, total: 15 }, focus: { label: 'Case 2', inputs: { s: 'in1' }, actual: 'a', expected: 'b' } });
  });

  it('given a Run of one bundled case that passes, when read, then it is accepted with that case\'s three values', () => {
    const result = consoleResultOf(planOf(1, { mode: 'run' }), respond([row({ actual: 'x', expected: 'x' })]));
    expect(result).toMatchObject({ kind: 'accepted', counts: null, focus: { label: 'Case 1', inputs: { s: 'in0' }, actual: 'x', expected: 'x' } });
  });

  it('given a custom case, when read, then it is custom with no counts, and the judge\'s pass false is ignored', () => {
    const result = consoleResultOf(planOf(1, { mode: 'run', custom: true }), respond([row({ pass: false, actual: 7, expected: null })]));
    expect(result).toMatchObject({ kind: 'custom', counts: null, inputs: { s: 'in0' }, result: 7 });
  });

  it('given case 4 raises with error_at line 3, when read, then it is a runtime error with the line and case 4\'s input', () => {
    const rows = [row(), row(), row(), row({ pass: false, error: 'IndexError: boom', error_at: { line: 3 }, actual: null })];
    const result = consoleResultOf(planOf(5), respond(rows));
    expect(result).toMatchObject({ kind: 'runtime-error', message: 'IndexError: boom', errorAt: { line: 3 }, fileName: 'Solution.py', lastInput: { label: 'Case 4', inputs: { s: 'in3' } } });
  });

  it('given a process-limit timeout after 6 rows, when read, then it is time-limit and the last input is the seventh case', () => {
    const response = runResponseOf({ ok: false, error_kind: 'timeout', error: 'Timeout: execution exceeded process limit', results: Array.from({ length: 6 }, () => row()) }, 'offline');
    expect(consoleResultOf(planOf(10), response)).toMatchObject({ kind: 'time-limit', lastInput: { label: 'Case 7' } });
  });

  it('given a row error that is the judge\'s case timeout, when read, then it is time-limit', () => {
    const rows = [row({ pass: false, error: 'TimeoutError: Code execution timed out (5s limit)', actual: null })];
    expect(consoleResultOf(planOf(2), respond(rows))).toMatchObject({ kind: 'time-limit', lastInput: { label: 'Case 1' } });
  });

  it('given a module-level timeout, when read, then it is time-limit with no last input', () => {
    const response = runResponseOf({ ok: false, error_kind: 'timeout', error: 'Timeout: module-level code exceeded the 5s limit', results: [] }, 'offline');
    expect(consoleResultOf(planOf(3), response)).toMatchObject({ kind: 'time-limit', lastInput: null });
  });

  it('given a compile error with error_at, when read, then it is a compile error with the line and no input', () => {
    const response = runResponseOf({ ok: false, error_kind: 'compile', error: 'SyntaxError: bad', error_at: { line: 2 }, results: [] }, 'offline');
    expect(consoleResultOf(planOf(3), response)).toMatchObject({ kind: 'compile-error', message: 'SyntaxError: bad', errorAt: { line: 2 }, lastInput: null });
  });

  it('given a crash with no rows, when read, then it is a runtime error with no input', () => {
    const response = runResponseOf({ ok: false, error_kind: 'runtime', error: 'The Python runtime stopped while running your code', results: [] }, 'offline');
    expect(consoleResultOf(planOf(3), response)).toMatchObject({ kind: 'runtime-error', lastInput: null });
  });

  it('given javascript, when an error is read, then the file name is Solution.js', () => {
    const response: RunResponse = runResponseOf({ ok: false, error_kind: 'compile', error: 'bad', results: [] }, 'offline');
    expect(consoleResultOf(planOf(1, { lang: 'javascript' }), response)).toMatchObject({ fileName: 'Solution.js' });
  });
});

describe('consoleResultOfFailure', () => {
  it('given a thrown runner error, when read, then it is run-failed with its message', () => {
    expect(consoleResultOfFailure(new CodeRunnerError('unavailable', 'no page'))).toEqual({ kind: 'run-failed', message: 'no page' });
  });
});

describe('isTimeoutText', () => {
  it('given the judge\'s two timeout texts and others, when classified, then only the timeouts match', () => {
    expect([isTimeoutText('TimeoutError: Code execution timed out (5s limit)'), isTimeoutText('Timeout: x'), isTimeoutText('ValueError: x')]).toEqual([true, true, false]);
  });
});
