import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { beforeAll, describe, expect, it, vi } from 'vitest';

import { consoleResultOf } from '../../../domain/console/consoleResult';
import type { PlannedCase, RunPlan } from '../../../domain/console/runPlan';
import { ProblemCatalog } from '../../../domain/problemCatalog';
import { runResponseOf } from '../../../domain/run/verdict';
import type { JudgePayload } from '../../runner/judgeProtocol';
import { bootNodeJudge, type NodeJudge } from './support/nodePyodide';
import { loadReferenceCorpus } from './support/referenceCorpus';

vi.setConfig({ testTimeout: 60_000, hookTimeout: 120_000 });

const named = JSON.parse(readFileSync(join(__dirname, 'fixtures/named.json'), 'utf8')) as Record<string, string>;
const goldens = loadReferenceCorpus();
const twoSum = ProblemCatalog.getById('two-sum');
const bundled = (twoSum?.testCases ?? []).slice(0, 4);
const plannedCases: PlannedCase[] = bundled.map((testCase, index) => ({ id: `bundled:${index}`, label: `Case ${index + 1}`, inputs: testCase.inputs }));

const verdictOf = (judge: NodeJudge, code: string, over: Partial<JudgePayload> = {}) => {
  const payload: JudgePayload = { code, methodName: 'twoSum', testCases: bundled, timeoutMs: 1000, ...over };
  const plan: RunPlan = {
    mode: 'submit',
    lang: 'python3',
    params: { lang: 'python3', code, methodName: 'twoSum', testCases: payload.testCases },
    cases: plannedCases,
    custom: false,
  };
  return consoleResultOf(plan, runResponseOf(judge.run(payload), 'offline', { surfaceCaseError: true }));
};

describe('console verdicts over the real judge', () => {
  let judge: NodeJudge;
  beforeAll(async () => {
    judge = await bootNodeJudge();
  });

  it('given a solution that loops forever in one case, when it runs with a one second limit, then it is Time Limit Exceeded at that case', () => {
    const result = verdictOf(judge, named.INFINITE_LOOP_ONE_CASE ?? '');
    expect(result).toMatchObject({ kind: 'time-limit', lastInput: { label: 'Case 2' } });
  });

  it('given a loop at module level, when it runs, then it is Time Limit Exceeded with no last input', () => {
    expect(verdictOf(judge, named.IMPORT_TIME_LOOP ?? '')).toMatchObject({ kind: 'time-limit', lastInput: null });
  });

  it('given a solution that raises on the third case, when it runs, then it is a Runtime Error with that line', () => {
    const raising = JSON.stringify(bundled[2]?.inputs.nums);
    const code = `class Solution:
    def twoSum(self, nums, target):
        if nums == ${raising}:
            raise IndexError("boom")
        for i in range(len(nums)):
            for j in range(i + 1, len(nums)):
                if nums[i] + nums[j] == target:
                    return [i, j]
`;
    expect(verdictOf(judge, code)).toMatchObject({ kind: 'runtime-error', message: 'IndexError: boom', errorAt: { line: 4 }, lastInput: { label: 'Case 3' } });
  });

  it('given a syntax error, when it runs, then it is a Compile Error with its line', () => {
    expect(verdictOf(judge, 'class Solution:\n    def twoSum(self, nums, target)\n        return []\n')).toMatchObject({ kind: 'compile-error', errorAt: { line: 2 } });
  });

  it('given an in-place problem whose solution changes nothing, when it runs, then it is Wrong Answer', () => {
    const problem = ProblemCatalog.getById('rotate-image');
    const code = 'class Solution:\n    def rotate(self, matrix):\n        pass\n';
    const cases = (problem?.testCases ?? []).slice(0, 2);
    const payload: JudgePayload = { code, methodName: problem?.methodName ?? '', testCases: cases, inPlace: problem?.inPlaceParam ?? null, timeoutMs: 1000 };
    const plan: RunPlan = {
      mode: 'submit',
      lang: 'python3',
      params: { lang: 'python3', ...payload, testCases: cases },
      cases: cases.map((testCase, index) => ({ id: `bundled:${index}`, label: `Case ${index + 1}`, inputs: testCase.inputs })),
      custom: false,
    };
    expect(consoleResultOf(plan, runResponseOf(judge.run(payload), 'offline', { surfaceCaseError: true }))).toMatchObject({ kind: 'wrong-answer' });
  });

  it('given the two-sum reference solution, when it runs, then it is accepted', () => {
    const code = goldens.find((g) => g.problem === 'two-sum')?.code ?? '';
    expect(verdictOf(judge, code, { timeoutMs: 5000 })).toMatchObject({ kind: 'accepted', counts: { correct: 4, total: 4 } });
  });
});
