/**
 * Code-execution domain types.
 *
 * These describe the contract between a problem's test cases and whatever
 * backend runs the code (the LeetCode judge bridge today; an offline runner in
 * principle). They are pure types/schemas with no React or React Native
 * dependencies so any layer can consume them.
 *
 * Moved here from the former `services/codeRunner` so the leetcode feature owns
 * its own contracts instead of reaching into a shared service bucket.
 */

import { z } from 'zod';

export const testCaseSchema = z.object({
  inputs: z.record(z.string(), z.unknown()),
  expected: z.unknown(),
  compareMode: z.enum(['exact', 'sorted', 'setOfSorted']).optional(),
});

export type TestCase = z.infer<typeof testCaseSchema>;

export type TestResult = {
  pass: boolean;
  input?: Record<string, unknown>;
  actual: unknown;
  expected: unknown;
  stdout: string;
  stderr: string;
  error: string | null;
  elapsed_ms: number;
};

export type RunResponse = {
  source: 'offline' | 'backend-run' | 'leetcode-submit' | 'leetcode-test';
  kind: 'submit' | 'test';
  state: 'FAILURE' | 'SUCCESS';
  requestId: string;
  run_success: boolean;
  status_msg: string;
  status_runtime?: string;
  total_correct: number;
  total_testcases: number;
  compile_error?: string;
  full_compile_error?: string;
  runtime_error?: string;
  code_answer?: string[];
  expected_code_answer?: string[];
  std_output_list?: string[];
  results?: TestResult[];
};

export type ExecResponse = {
  type: 'exec-result';
  requestId: string;
  stdout: string;
  stderr: string;
  error: string | null;
  elapsed_ms: number;
};

export type ExecutionBackend = {
  readonly supportedLanguages: string[];
  runCode: (
    code: string,
    methodName: string,
    testCases: TestCase[]
  ) => Promise<RunResponse>;
  isReady: () => boolean;
};
