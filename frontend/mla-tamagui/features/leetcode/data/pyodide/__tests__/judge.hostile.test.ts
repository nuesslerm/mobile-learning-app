import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { beforeAll, describe, expect, it, vi } from 'vitest';

import { ProblemCatalog } from '../../../domain/problemCatalog';
import type { JudgePayload } from '../judgeProtocol';
import { bootNodeJudge, type NodeJudge } from './support/nodePyodide';

vi.setConfig({ testTimeout: 60_000, hookTimeout: 120_000 });

const named = JSON.parse(readFileSync(join(__dirname, 'fixtures/named.json'), 'utf8')) as Record<string, string>;
const cases = (n: number) => (ProblemCatalog.getById('two-sum')?.testCases ?? []).slice(0, n);
const payload = (code: string, extra: Partial<JudgePayload> = {}): JudgePayload => ({ code, methodName: 'twoSum', testCases: cases(4), timeoutMs: 1000, ...extra });

describe('the on-device judge under hostile code', () => {
  let judge: NodeJudge;
  beforeAll(async () => {
    judge = await bootNodeJudge();
  });

  it('given an infinite loop in one case, when it runs with a one second limit, then that row alone times out and the others pass', () => {
    const rows = judge.run(payload(named.INFINITE_LOOP_ONE_CASE ?? '')).results ?? [];
    expect(rows.map((r) => r.error)).toEqual([null, 'TimeoutError: Code execution timed out (1s limit)', null, null]);
  });

  it('given a loop that swallows every Exception, when it runs, then the deadline still ends it', () => {
    const rows = judge.run(payload(named.SWALLOWS_TIMEOUT ?? '', { testCases: cases(1) })).results ?? [];
    expect(rows[0]?.error).toBe('TimeoutError: Code execution timed out (1s limit)');
  });

  it('given a loop at module level, when it runs, then the run fails as a timeout, not a 30 second hang', () => {
    const raw = judge.run(payload(named.IMPORT_TIME_LOOP ?? ''));
    expect(raw).toMatchObject({ ok: false, error_kind: 'timeout', error: 'Timeout: module-level code exceeded the 1s limit' });
  });

  it('given a solution that raises the recursion limit and recurses forever, when it runs, then it is a RecursionError row, fast, and the limit is back', () => {
    const code = 'import sys\nclass Solution:\n    def twoSum(self, nums, target):\n        sys.setrecursionlimit(10**6)\n        def f(n):\n            return f(n + 1)\n        return f(0)\n';
    const started = Date.now();
    const rows = judge.run(payload(code, { testCases: cases(2) })).results ?? [];
    expect(rows.map((r) => r.error?.split(':')[0])).toEqual(['RecursionError', 'RecursionError']);
    expect(Date.now() - started).toBeLessThan(8000);
    expect(judge.py.runPython('import sys; sys.getrecursionlimit()')).toBe(1000);
  });

  it('given a solution that prints 200,000 lines, when it runs, then the captured output is capped', () => {
    const code = 'class Solution:\n    def twoSum(self, nums, target):\n        for i in range(200000):\n            print("line", i)\n        return [0, 1]\n';
    const row = judge.run(payload(code, { testCases: cases(1), timeoutMs: 20_000 })).results?.[0];
    expect(row?.stdout.length).toBeLessThanOrEqual(262_166);
    expect(row?.stdout.endsWith('...[output truncated]')).toBe(true);
  });

  it('given a solution that returns infinity, when it runs, then it is a row, not a crashed run', () => {
    const code = 'class Solution:\n    def twoSum(self, nums, target):\n        return float("inf")\n';
    expect(judge.run(payload(code, { testCases: cases(1) })).results?.[0]?.actual).toBe('inf');
  });

  it('given cases that each take the whole limit, when the whole-run limit is shorter, then the run stops with the process-limit message and every finished row was emitted', () => {
    const code = 'class Solution:\n    def twoSum(self, nums, target):\n        while True:\n            pass\n';
    const seen: string[] = [];
    const raw = judge.run(payload(code, { testCases: cases(8), processLimitMs: 2500 }), (row) => seen.push(row));
    expect(raw).toMatchObject({ ok: false, error_kind: 'timeout', error: 'Timeout: execution exceeded process limit' });
    expect(seen).toHaveLength(3);
    expect(seen.map((row) => (JSON.parse(row) as { index: number }).index)).toEqual([0, 1, 2]);
  });

  it('given a run, when its cases finish, then each row is emitted once in order as the run goes', () => {
    const seen: string[] = [];
    judge.run(payload(named.TWO_SUM ?? '', { testCases: cases(3) }), (row) => seen.push(row));
    expect(seen.map((row) => (JSON.parse(row) as { index: number }).index)).toEqual([0, 1, 2]);
  });
});
