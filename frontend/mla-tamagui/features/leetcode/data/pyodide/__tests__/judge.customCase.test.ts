import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { beforeAll, describe, expect, it, vi } from 'vitest';

import { ProblemCatalog } from '../../../domain/problemCatalog';
import { runParamsFor } from '../../../domain/run/runParams';
import type { JudgePayload } from '../judgeProtocol';
import { bootNodeJudge, type NodeJudge } from './support/nodePyodide';

vi.setConfig({ testTimeout: 60_000, hookTimeout: 120_000 });

const goldens = JSON.parse(readFileSync(join(__dirname, 'fixtures/referenceSolutions.json'), 'utf8')) as { problem: string; code: string }[];
const named = JSON.parse(readFileSync(join(__dirname, 'fixtures/named.json'), 'utf8')) as Record<string, string>;
const solutionFor = (problemId: string) => goldens.find((g) => g.problem === problemId)?.code ?? '';

/** What the screen sends for a typed case: the problem's own method and node types, the one case with no expected value. */
function typedCase(problemId: string, code: string, inputs: Record<string, unknown>): JudgePayload {
  const problem = ProblemCatalog.getById(problemId);
  if (!problem) throw new Error(`no problem ${problemId}`);
  const { lang: _lang, ...payload } = runParamsFor(problem, 'python3', code, [{ inputs, expected: null }]);
  return payload;
}

const TWO_SUM_INPUT = { nums: [2, 7, 11, 15], target: 9 };

describe('the judge on one typed case with no expected value', () => {
  let judge: NodeJudge;
  beforeAll(async () => {
    judge = await bootNodeJudge();
  });

  it('given the two-sum solution and a typed case with expected null, when it is judged, then the one row holds the value, the input as typed and no error', () => {
    const raw = judge.run(typedCase('two-sum', named.TWO_SUM ?? '', TWO_SUM_INPUT));
    expect(raw.ok).toBe(true);
    expect(raw.results).toHaveLength(1);
    expect(raw.results?.[0]).toMatchObject({ actual: [0, 1], error: null, input: TWO_SUM_INPUT, expected: null, stdout: '' });
    expect(raw.results?.[0]?.elapsed_ms).toBeGreaterThanOrEqual(0);
  });

  it('given typed lists, trees with nulls and in-place matrices, when they are judged with the catalog paramTypes and returnType, then the values come back as lists', () => {
    const answers = [
      judge.run(typedCase('reverse-linked-list', solutionFor('reverse-linked-list'), { head: [1, 2, 3] })),
      judge.run(typedCase('invert-binary-tree', solutionFor('invert-binary-tree'), { root: [4, 2, 7, null, 3] })),
      judge.run(typedCase('rotate-image', solutionFor('rotate-image'), { matrix: [[1, 2], [3, 4]] })),
    ].map((raw) => raw.results?.[0]);
    expect(answers.map((row) => row?.error)).toEqual([null, null, null]);
    expect(answers.map((row) => row?.actual)).toEqual([[3, 2, 1], [4, 7, 2, null, null, 3], [[3, 1], [4, 2]]]);
  });

  it('given a solution that raises, prints, or is handed too few arguments, when it is judged, then the row says so and the run still completes', () => {
    const body = (line: string) => `class Solution:\n    def twoSum(self, nums, target):\n${line}\n        return [0, 1]\n`;

    const raised = judge.run(typedCase('two-sum', body("        raise IndexError('boom')"), TWO_SUM_INPUT));
    expect(raised.ok).toBe(true);
    expect(raised.results?.[0]).toMatchObject({ actual: null, error: 'IndexError: boom', error_at: { line: 3 } });

    const printed = judge.run(typedCase('two-sum', body("        print('hello', nums)"), TWO_SUM_INPUT));
    expect(printed.results?.[0]).toMatchObject({ actual: [0, 1], error: null, stdout: 'hello [2, 7, 11, 15]\n' });

    const short = judge.run(typedCase('two-sum', named.TWO_SUM ?? '', { nums: [2, 7] }));
    expect(short.ok).toBe(true);
    expect(short.results?.[0]?.error).toMatch(/^TypeError: .*target/);
  });

  it("given the same case through the backend's verbatim harness, when it runs, then it answers with the value and no error but says pass false", () => {
    // pass compares the value with a null that stands for nothing: this is why the card never reads it
    const raw = judge.runBackend(typedCase('two-sum', named.TWO_SUM ?? '', TWO_SUM_INPUT));
    expect(raw.ok).toBe(true);
    expect(raw.results).toHaveLength(1);
    expect(raw.results?.[0]).toMatchObject({ actual: [0, 1], error: null, pass: false, expected: null });
  });
});
