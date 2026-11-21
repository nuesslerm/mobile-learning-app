import { z } from 'zod';

import { errorLocationSchema, testResultSchema, type RunResponse } from './execution';

/** What a runner reports about one run, before it is shaped into a `RunResponse`: the leetcode-backend's `/run` payload and the on-device harness's. */
export const rawRunSchema = z.object({
  ok: z.boolean().optional(),
  error: z.string().optional(),
  error_kind: z.enum(['compile', 'runtime', 'timeout']).optional(),
  error_at: errorLocationSchema.optional(),
  results: z.array(testResultSchema).optional(),
  total: z.number().int().nonnegative().optional(),
});
export type RawRun = z.infer<typeof rawRunSchema>;

const LOCATION_SUFFIX = / \(<string>, line (\d+)\)$/;

/**
 * The one place a run's raw report becomes the verdict the screen reads, so a local run and a remote one cannot disagree about
 * what "Accepted", "Wrong Answer" or an error is. `surfaceCaseError` lifts the first raising case's error and line into the
 * response (the on-device runner sets it: nothing else tells the learner why a case raised).
 */
export function runResponseOf(raw: RawRun, source: RunResponse['source'], options: { surfaceCaseError?: boolean } = {}): RunResponse {
  const results = raw.results ?? [];
  const totalCorrect = results.filter((r) => r.pass).length;
  const totalTestcases = raw.total ?? results.length;
  const base = { source, kind: 'test' as const, requestId: '', total_correct: totalCorrect, total_testcases: totalTestcases, results };
  if (!raw.ok || raw.error) {
    const message = raw.error ?? 'Unknown error';
    const kind = raw.error_kind ?? (message.includes('Timeout') ? 'timeout' : 'compile');
    // the backend gives no location, but str(SyntaxError) in its Python harness ends with one
    const printed = kind === 'compile' && !raw.error_at ? LOCATION_SUFFIX.exec(message) : null;
    const errorAt = raw.error_at ?? (printed ? { line: Number(printed[1]) } : undefined);
    const text = kind === 'compile' && errorAt ? message.replace(LOCATION_SUFFIX, '') : message;
    return {
      ...base,
      state: 'FAILURE',
      run_success: false,
      status_msg: 'Error',
      ...(kind === 'compile' ? { compile_error: text } : { runtime_error: text }),
      ...(errorAt ? { error_at: errorAt } : {}),
    };
  }
  const accepted = totalCorrect === totalTestcases;
  const raised = options.surfaceCaseError && !accepted ? results.find((r) => r.error !== null) : undefined;
  return {
    ...base,
    state: accepted ? 'SUCCESS' : 'FAILURE',
    run_success: true,
    status_msg: accepted ? 'Accepted' : 'Wrong Answer',
    ...(raised?.error ? { runtime_error: raised.error } : {}),
    ...(raised?.error_at ? { error_at: raised.error_at } : {}),
  };
}
