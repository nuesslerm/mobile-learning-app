import type { RunResponse, TestResult } from '../domain/execution';
import type { CodeRunnerPort, RunCodeParams } from '../domain/ports';

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
