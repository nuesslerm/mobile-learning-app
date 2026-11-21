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
  compareMode: z.enum(['exact', 'sorted', 'setOfSorted', 'approx']).optional(),
});

export type TestCase = z.infer<typeof testCaseSchema>;

export const errorLocationSchema = z.object({
  line: z.number().int(),
  column: z.number().int().optional(),
});
/** Where in the learner's code an error was raised, 1-based: the editor jumps to it. */
export type ErrorLocation = z.infer<typeof errorLocationSchema>;

export type TestResult = {
  pass: boolean;
  input?: Record<string, unknown>;
  actual: unknown;
  expected: unknown;
  stdout: string;
  stderr: string;
  error: string | null;
  error_at?: ErrorLocation;
  elapsed_ms: number;
};

export const testResultSchema = z.object({
  pass: z.boolean(),
  input: z.record(z.string(), z.unknown()).optional(),
  actual: z.unknown(),
  expected: z.unknown(),
  stdout: z.string(),
  stderr: z.string(),
  error: z.string().nullable(),
  error_at: errorLocationSchema.optional(),
  elapsed_ms: z.number(),
}) satisfies z.ZodType<TestResult>;

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
  /** The line of the compile or runtime error, when the runner can tell. */
  error_at?: ErrorLocation;
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
