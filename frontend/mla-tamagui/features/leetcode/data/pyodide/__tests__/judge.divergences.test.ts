import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { beforeAll, describe, expect, it, vi } from 'vitest';

import { ProblemCatalog } from '../../../domain/problemCatalog';
import type { JudgePayload } from '../judgeProtocol';
import { bootNodeJudge, type NodeJudge } from './support/nodePyodide';

vi.setConfig({ testTimeout: 60_000, hookTimeout: 120_000 });

const named = JSON.parse(readFileSync(join(__dirname, 'fixtures/named.json'), 'utf8')) as Record<string, string>;
const twoSumCases = (n: number) => (ProblemCatalog.getById('two-sum')?.testCases ?? []).slice(0, n);
const twoSum = (code: string, cases = 3): JudgePayload => ({ code, methodName: 'twoSum', testCases: twoSumCases(cases) });

/**
 * Where the backend breaks and the local judge does the sensible thing. Each divergence is pinned on both sides, so a change on either
 * one is a decision and not a surprise.
 */
describe('the deliberate differences from the backend', () => {
  let judge: NodeJudge;
  beforeAll(async () => {
    judge = await bootNodeJudge();
  });

  it('given a print at module level, when it runs, then the backend loses the whole run and the judge puts the text in the first case’s stdout', () => {
    const remote = judge.runBackend(twoSum(named.MODULE_PRINT ?? ''));
    const local = judge.run(twoSum(named.MODULE_PRINT ?? ''));
    expect(remote.error).toBe('Runner produced no parseable output');
    expect(local.results?.[0]?.stdout).toContain('module level');
  });

  it('given sys.exit in the solution, when it runs, then the backend loses the run and the judge reports a row per case', () => {
    expect(judge.runBackend(twoSum(named.SYS_EXIT ?? '')).error).toBe('Runner produced no parseable output');
    expect(judge.run(twoSum(named.SYS_EXIT ?? '')).results?.map((r) => r.error?.split(':')[0])).toEqual(['SystemExit', 'SystemExit', 'SystemExit']);
  });

  it('given a solution that returns NaN, when it runs, then the backend loses the run and the judge reports the text nan', () => {
    expect(judge.runBackend(twoSum(named.RETURNS_NAN ?? '')).error).toBe('Runner produced no parseable output');
    expect(judge.run(twoSum(named.RETURNS_NAN ?? '')).results?.[0]?.actual).toBe('nan');
  });

  it('given a solution that mutates its input, when it runs, then the judge shows the input the case had and the backend shows the mutated one', () => {
    const original = twoSumCases(2)[1]?.inputs;
    expect(judge.run(twoSum(named.TWO_SUM_MUTATES_INPUT ?? '', 2)).results?.[1]?.input).toEqual(original);
    expect(judge.runBackend(twoSum(named.TWO_SUM_MUTATES_INPUT ?? '', 2)).results?.[1]?.input).not.toEqual(original);
  });

  it('given annotations naming something never imported, when it runs, then it runs here because Python 3.14 reads annotations lazily (the backend’s 3.11 refuses it)', () => {
    const code = 'class Solution:\n    def twoSum(self, nums: List[int], target: int) -> Counter:\n        return [0, 1]\n';
    expect(judge.run(twoSum(code, 1)).ok).toBe(true);
  });

  it('given a raised error in the learner’s code, when it runs, then the row says where: the line, and the column in characters', () => {
    const row = judge.run(twoSum(named.EXCEPTION_AT_COLUMN ?? '', 1)).results?.[0];
    expect(row?.error).toBe("NameError: name 'undefined_name' is not defined");
    expect(row?.error_at).toEqual({ line: 4, column: 26 });
  });

  it('given a syntax error, when it runs, then the run fails as a compile error with the line the editor can jump to', () => {
    const raw = judge.run(twoSum(named.SYNTAX_ERROR ?? ''));
    expect(raw).toMatchObject({ ok: false, error_kind: 'compile' });
    expect(raw.error_at?.line).toBeGreaterThan(0);
  });

  it('given a multi-byte character before the failing token on its line, when it runs, then the column still counts characters', () => {
    const source = named.EXCEPTION_AFTER_ACCENT ?? '';
    const row = judge.run(twoSum(source, 1)).results?.[0];
    const line = source.split('\n')[2] ?? '';
    expect(row?.error_at).toEqual({ line: 3, column: line.indexOf('undefined_name') + 1 });
  });

  it('given a syntax error raised at run time by eval, when it runs, then the row points at the learner’s line and not at line 1 of the snippet', () => {
    const row = judge.run(twoSum(named.EVAL_SYNTAX_ERROR ?? '', 1)).results?.[0];
    expect(row?.error).toMatch(/^SyntaxError/);
    expect(row?.error_at?.line).toBe(4);
  });

  it('given an error raised inside code a library generated with exec, when it runs, then the row points at the learner’s own line', () => {
    const source = named.DATACLASS_ORDER_ERROR ?? '';
    const row = judge.run(twoSum(source, 1)).results?.[0];
    expect(row?.error).toMatch(/^TypeError/);
    expect(row?.error_at?.line).toBe(7);
    expect(row?.error_at?.line).toBeLessThanOrEqual(source.split('\n').length);
  });

  it('given a solution that closes sys.stdout, when it runs, then every case still gets its row and the run is not lost', () => {
    const raw = judge.run(twoSum(named.CLOSES_STDOUT ?? '', 3));
    expect(raw.ok).toBe(true);
    expect(raw.results).toHaveLength(3);
  });

  it('given an exception whose own text cannot be produced, when it runs, then the row names the exception and the run goes on', () => {
    const raw = judge.run(twoSum(named.UNPRINTABLE_EXCEPTION ?? '', 2));
    expect(raw.ok).toBe(true);
    expect(raw.results?.map((r) => r.error)).toEqual(['Boom: <unprintable>', 'Boom: <unprintable>']);
  });

  it('given an error raised inside the learner’s own eval, when it runs, then the row points at the line that called eval', () => {
    const row = judge.run(twoSum(named.EVAL_RUNTIME_ERROR ?? '', 1)).results?.[0];
    expect(row?.error).toBe('ZeroDivisionError: division by zero');
    expect(row?.error_at?.line).toBe(3);
  });

  describe('the approx compare mode, which the backend does not have', () => {
    const answer = (body: string, cases: { x: unknown; expected: unknown; compareMode?: 'exact' | 'approx' }[]): JudgePayload => ({
      code: `class Solution:\n    def answer(self, x):\n        ${body}\n`,
      methodName: 'answer',
      testCases: cases.map(({ x, expected, compareMode }) => ({ inputs: { x }, expected, compareMode })),
    });
    const passes = (payload: JudgePayload) => judge.run(payload).results?.map((r) => r.pass);

    it('given a median that returns 2.0 where the case expects 2, when it runs in approx mode, then the row passes', () => {
      expect(passes(answer('return float(x)', [{ x: 2, expected: 2, compareMode: 'approx' }]))).toEqual([true]);
    });

    it('given 1.5000001 against 1.5 in approx mode, when it runs, then it passes, and given 1.5001 then it fails', () => {
      const cases = [{ x: 1.5000001, expected: 1.5, compareMode: 'approx' as const }, { x: 1.5001, expected: 1.5, compareMode: 'approx' as const }];
      expect(passes(answer('return x', cases))).toEqual([true, false]);
    });

    it('given lists of floats in approx mode, when they run, then each element is compared within the tolerance and a different length fails', () => {
      const cases = [
        { x: [1.0000001, 2.5], expected: [1, 2.5], compareMode: 'approx' as const },
        { x: [1, 2.5, 3], expected: [1, 2.5], compareMode: 'approx' as const },
        { x: [1, 2.6], expected: [1, 2.5], compareMode: 'approx' as const },
      ];
      expect(passes(answer('return x', cases))).toEqual([true, false, false]);
    });

    it('given True against 1 in approx mode, when it runs, then it fails because a bool is not a number, and the same for 1 against True', () => {
      expect(passes(answer('return True', [{ x: 0, expected: 1, compareMode: 'approx' }]))).toEqual([false]);
      expect(passes(answer('return 1', [{ x: 0, expected: true, compareMode: 'approx' }]))).toEqual([false]);
      expect(passes(answer('return True', [{ x: 0, expected: true, compareMode: 'approx' }]))).toEqual([true]);
    });

    it('given None or NaN against a number in approx mode, when it runs, then it fails', () => {
      expect(passes(answer('return None', [{ x: 0, expected: 1, compareMode: 'approx' }]))).toEqual([false]);
      expect(passes(answer("return float('nan')", [{ x: 0, expected: 1, compareMode: 'approx' }]))).toEqual([false]);
      expect(passes(answer('return None', [{ x: 0, expected: null, compareMode: 'approx' }]))).toEqual([true]);
    });

    it('given 2.0 against 2 in exact mode, when it runs, then it still fails because the backend-parity semantics are unchanged', () => {
      expect(passes(answer('return float(x)', [{ x: 2, expected: 2 }, { x: 2, expected: 2, compareMode: 'exact' }]))).toEqual([false, false]);
    });

    it('given an approx case, when the backend’s own script runs it, then it compares exactly and fails where the judge passes', () => {
      const payload = answer('return float(x)', [{ x: 2, expected: 2, compareMode: 'approx' }]);
      expect(judge.run(payload).results?.[0]?.pass).toBe(true);
      expect(judge.runBackend(payload).results?.[0]?.pass).toBe(false);
    });
  });
});
