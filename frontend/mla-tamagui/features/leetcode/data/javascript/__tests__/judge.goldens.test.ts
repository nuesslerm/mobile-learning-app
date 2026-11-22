import { describe, expect, it, vi } from 'vitest';

import { ProblemCatalog } from '../../../domain/problemCatalog';
import { runParamsFor } from '../../../domain/run/runParams';
import { runResponseOf } from '../../../domain/run/verdict';
import type { JudgePayload } from '../../runner/judgeProtocol';
import { judge } from './support/judgeRun';
import { loadReferenceCorpus } from './support/referenceCorpus';

vi.setConfig({ testTimeout: 120_000 });

/** group-anagrams: the standard solution fails one case whose outer order the catalog fixed, and the backend scores it the same, so it is pinned by the Python goldens and has no reference here either. */
const NO_REFERENCE = ['group-anagrams'];

/** The catalog problems that have no JavaScript reference yet. A batch of references removes its ids from this list; the list is deleted when it is empty. */
const PENDING_REFERENCES = [
  '3sum', 'alien-dictionary', 'best-time-to-buy-and-sell-stock',
  'binary-tree-level-order-traversal', 'binary-tree-maximum-path-sum', 'climbing-stairs',
  'coin-change', 'combination-sum', 'combination-sum-iv',
  'construct-binary-tree-from-preorder-and-inorder-traversal', 'container-with-most-water', 'contains-duplicate',
  'counting-bits', 'course-schedule', 'decode-ways',
  'design-add-and-search-words-data-structure', 'encode-and-decode-strings', 'find-median-from-data-stream',
  'find-minimum-in-rotated-sorted-array', 'graph-valid-tree', 'house-robber',
  'house-robber-ii', 'insert-interval', 'jump-game',
  'kth-smallest-element-in-a-bst', 'longest-common-subsequence', 'longest-consecutive-sequence',
  'longest-increasing-subsequence', 'longest-palindromic-substring', 'longest-repeating-character-replacement',
  'longest-substring-without-repeating-characters', 'lowest-common-ancestor-of-a-binary-search-tree', 'maximum-depth-of-binary-tree',
  'maximum-product-subarray', 'maximum-subarray', 'meeting-rooms',
  'meeting-rooms-ii', 'merge-intervals', 'merge-k-sorted-lists',
  'merge-two-sorted-lists', 'minimum-window-substring', 'missing-number',
  'non-overlapping-intervals', 'number-of-1-bits', 'number-of-connected-components-in-an-undirected-graph',
  'number-of-islands', 'pacific-atlantic-water-flow', 'palindromic-substrings',
  'product-of-array-except-self', 'remove-nth-node-from-end-of-list', 'reorder-list',
  'reverse-bits', 'same-tree', 'set-matrix-zeroes',
  'spiral-matrix', 'subtree-of-another-tree', 'sum-of-two-integers',
  'top-k-frequent-elements', 'unique-paths', 'valid-anagram',
  'validate-binary-search-tree', 'word-break', 'word-search',
  'word-search-ii',
];

const goldens = loadReferenceCorpus();

function payloadFor(problemId: string, code: string): JudgePayload {
  const { lang: _lang, ...payload } = runParamsFor(ProblemCatalog.getByIdOrThrow(problemId), 'javascript', code);
  return payload;
}

describe('the on-device JavaScript judge on known-good solutions', () => {
  it('Given the catalog, When the pending list, the pinned exception and the corpus are compared, Then the problems with no reference are exactly the pending list', () => {
    const covered = new Set(goldens.map(({ problem }) => problem));
    const uncovered = ProblemCatalog.getAll()
      .map((problem) => problem.id)
      .filter((id) => !covered.has(id) && !NO_REFERENCE.includes(id));

    // The failure names every problem that has no known-good solution and is not on the list, and every one on the list that now has one
    expect([...uncovered].sort()).toEqual([...PENDING_REFERENCES].sort());
  });

  it('Given the reference of every covered problem, When it runs with the catalog’s paramTypes, returnType, inPlace and harness, Then every bundled case passes', async () => {
    const failures: string[] = [];
    let cases = 0;
    for (const { problem, code } of goldens) {
      const { raw } = await judge(payloadFor(problem, code));
      const response = runResponseOf(raw, 'offline');
      cases += response.total_testcases;
      if (response.status_msg !== 'Accepted' || response.total_testcases === 0) {
        failures.push(`${problem}: ${response.status_msg} ${response.total_correct}/${response.total_testcases} ${response.compile_error ?? response.runtime_error ?? ''}`);
      }
    }
    expect(failures).toEqual([]);
    expect(cases).toBe(goldens.reduce((sum, { problem }) => sum + (ProblemCatalog.getByIdOrThrow(problem).testCases?.length ?? 0), 0));
  });

  it('Given the starter of each covered problem, When it runs as given, Then it is never Accepted and never a runner crash', async () => {
    for (const { problem } of goldens) {
      const starter = ProblemCatalog.getByIdOrThrow(problem).starterCode.javascript ?? '';
      const { raw } = await judge(payloadFor(problem, starter));
      const response = runResponseOf(raw, 'offline');
      expect(response.status_msg, problem).not.toBe('Accepted');
      expect(`${response.compile_error ?? ''}${response.runtime_error ?? ''}`, problem).not.toMatch(/Runner error/);
    }
  });
});
