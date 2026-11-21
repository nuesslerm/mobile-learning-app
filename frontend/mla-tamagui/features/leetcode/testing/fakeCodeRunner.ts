import type { RunResponse, TestResult } from '../domain/run/execution';
import type { CodeRunnerPort, RunCodeParams } from '../domain/ports';
import { runResponseOf } from '../domain/run/verdict';

export type FakeCodeRunner = CodeRunnerPort & { readonly calls: RunCodeParams[] };

const passingCase = (n: number): TestResult => ({
  pass: true,
  input: { case: n },
  actual: n,
  expected: n,
  stdout: '',
  stderr: '',
  error: null,
  elapsed_ms: 1,
});

/** A run whose cases all pass. */
export function passingRun(total = 3): RunResponse {
  return {
    source: 'offline',
    kind: 'test',
    state: 'SUCCESS',
    requestId: 'fake-runner',
    run_success: true,
    status_msg: 'Accepted',
    total_correct: total,
    total_testcases: total,
    results: Array.from({ length: total }, (_, n) => passingCase(n)),
  };
}

/** A run of `total` cases whose last one fails with a wrong answer. */
export function failingRun(total = 3): RunResponse {
  const results = Array.from({ length: total }, (_, n) => passingCase(n));
  results[total - 1] = { ...results[total - 1]!, pass: false, actual: -1, stdout: 'saw -1\n' };
  return {
    source: 'offline',
    kind: 'test',
    state: 'FAILURE',
    requestId: 'fake-runner',
    run_success: true,
    status_msg: 'Wrong Answer',
    total_correct: total - 1,
    total_testcases: total,
    results,
  };
}

/**
 * What the real harness answers for one typed case: a row whose `pass` is false because nothing was compared (`expected` is null),
 * so a card that read the verdict would show a failure.
 */
export function customRun(params: RunCodeParams, over: Partial<TestResult> = {}): RunResponse {
  const row: TestResult = { pass: false, input: params.testCases[0]?.inputs ?? {}, actual: [0, 1], expected: null, stdout: '', stderr: '', error: null, elapsed_ms: 1.5, ...over };
  return runResponseOf({ ok: true, results: [row] }, 'offline', { surfaceCaseError: true });
}

/** A runner that answers every run with `answer` (default: all cases pass) and remembers what it was asked. */
export function createFakeCodeRunner(answer: (p: RunCodeParams) => RunResponse | Promise<RunResponse> = () => passingRun()): FakeCodeRunner {
  const calls: RunCodeParams[] = [];
  return {
    calls,
    run: async (params) => {
      calls.push(params);
      return answer(params);
    },
  };
}
