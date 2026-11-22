import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { beforeAll, describe, expect, it, vi } from 'vitest';

import { ProblemCatalog } from '../../../domain/problemCatalog';
import { runParamsFor } from '../../../domain/run/runParams';
import { runResponseOf } from '../../../domain/run/verdict';
import type { JudgePayload } from '../../runner/judgeProtocol';
import { bootNodeJudge, type NodeJudge } from './support/nodePyodide';
import { BRUTE_DIR, loadReferenceCorpus, referenceFor } from './support/referenceCorpus';

vi.setConfig({ testTimeout: 60_000, hookTimeout: 120_000 });

const COMPUTED_PROBLEMS = [
  'alien-dictionary',
  'design-add-and-search-words-data-structure',
  'find-median-from-data-stream',
  'implement-trie-prefix-tree',
  'linked-list-cycle',
  'longest-palindromic-substring',
  'maximum-product-subarray',
  'palindromic-substrings',
  'reorder-list',
];
/** group-anagrams: the standard solution fails one case whose outer order the catalog fixed, and the backend scores it the same, so the named test below pins it instead of a reference. */
const NO_REFERENCE = ['group-anagrams'];
const ADDED_BY_DATA_SLICES = [...COMPUTED_PROBLEMS, 'encode-and-decode-strings', 'serialize-and-deserialize-binary-tree', 'clone-graph'];

const goldens = loadReferenceCorpus();
const named = JSON.parse(readFileSync(join(__dirname, 'fixtures/named.json'), 'utf8')) as Record<string, string>;

function payloadFor(problemId: string, code: string): JudgePayload {
  const problem = ProblemCatalog.getById(problemId);
  if (!problem) throw new Error(`no problem ${problemId}`);
  const { lang: _lang, ...payload } = runParamsFor(problem, 'python3', code);
  return payload;
}

describe('the on-device judge on known-good solutions', () => {
  let judge: NodeJudge;
  beforeAll(async () => {
    judge = await bootNodeJudge();
  });

  it('given every problem in the catalog, when the corpus is listed, then each has a reference file and the only exception is the problem whose standard solution fails a pinned case', () => {
    const covered = new Set(goldens.map(({ problem }) => problem));

    // The failure lists every catalog problem that has no known-good solution yet
    expect(ProblemCatalog.getAll().map((problem) => problem.id).filter((id) => !covered.has(id))).toEqual(NO_REFERENCE);
  });

  it('given the reference solution of every catalog problem, when it runs with the catalog paramTypes, returnType, inPlaceParam and harness, then every bundled case passes', () => {
    const failures: string[] = [];
    let cases = 0;
    for (const { problem, code } of goldens) {
      // When each runs through the judge
      const raw = judge.run(payloadFor(problem, code));
      const response = runResponseOf(raw, 'offline');

      // Then it is Accepted over every bundled case
      cases += response.total_testcases;
      if (response.status_msg !== 'Accepted' || response.total_testcases === 0) failures.push(`${problem}: ${response.status_msg} ${response.total_correct}/${response.total_testcases} ${response.compile_error ?? response.runtime_error ?? ''}`);
    }
    expect(failures).toEqual([]);
    expect(cases).toBe(ProblemCatalog.getAll().filter((problem) => !NO_REFERENCE.includes(problem.id)).reduce((sum, problem) => sum + (problem.testCases?.length ?? 0), 0));
  });

  it('given the brute-force oracle of each problem whose expected values were computed, when it runs with the catalog harness, then every case passes', () => {
    // Given the independent second opinions: a set of words, a regex over a list, a sort, a visited set, a pointer array, every substring, every letter order
    const oracles = loadReferenceCorpus(BRUTE_DIR);
    expect(oracles.map((oracle) => oracle.problem).sort()).toEqual([...COMPUTED_PROBLEMS].sort());
    for (const { problem, code } of oracles) {
      // When each runs through the judge
      const response = runResponseOf(judge.run(payloadFor(problem, code)), 'offline');

      // Then every bundled case passes, so no expected value rests on the reference alone
      expect(response, problem).toMatchObject({ status_msg: 'Accepted', total_correct: ProblemCatalog.getByIdOrThrow(problem).testCases?.length });
    }
  });

  it('given the oracle of a problem with several acceptable answers and an input that has two, when it runs, then the row is an error that says so, so no such input can be a bundled case', () => {
    const longest = runResponseOf(judge.run({ ...payloadFor('longest-palindromic-substring', loadReferenceCorpus(BRUTE_DIR).find((o) => o.problem === 'longest-palindromic-substring')?.code ?? ''), testCases: [{ inputs: { s: 'babad' }, expected: 'bab' }] }), 'offline');
    const alien = runResponseOf(judge.run({ ...payloadFor('alien-dictionary', loadReferenceCorpus(BRUTE_DIR).find((o) => o.problem === 'alien-dictionary')?.code ?? ''), testCases: [{ inputs: { words: ['ab', 'adc'] }, expected: 'abcd' }] }), 'offline');

    expect(longest.results?.[0]?.error).toMatch(/longest palindrome is not unique/);
    expect(alien.results?.[0]?.error).toMatch(/more than one order fits/);
  });

  it('given the starter of each problem the data slices added cases to, when it runs as given, then it is never Accepted and never a runner crash', () => {
    for (const id of ADDED_BY_DATA_SLICES) {
      // Given the starter exactly as the editor first shows it
      const starter = ProblemCatalog.getByIdOrThrow(id).starterCode.python3 ?? '';

      // When it runs
      const response = runResponseOf(judge.run(payloadFor(id, starter)), 'offline');

      // Then the learner sees a wrong answer or an error that names the starter's own line
      expect(response.status_msg, id).not.toBe('Accepted');
      expect(`${response.compile_error ?? ''}${response.runtime_error ?? ''}`, id).not.toMatch(/Runner error|No \w+ class defined/);
    }
  });

  it('given the in-place problems, when their reference solutions run, then the mutated argument is what is compared and they pass where the backend cannot', () => {
    for (const id of ['rotate-image', 'set-matrix-zeroes']) {
      const code = referenceFor(id, goldens);
      const local = runResponseOf(judge.run(payloadFor(id, code)), 'offline');
      const remote = runResponseOf(judge.runBackend({ ...payloadFor(id, code), paramTypes: undefined, returnType: undefined, inPlace: undefined }), 'backend-run');
      expect(local.status_msg, id).toBe('Accepted');
      expect(remote.total_correct, `${id} on the backend`).toBe(0);
    }
  });

  it('given a linked-list problem, when the backend runs the same correct solution, then it scores zero because it hands the user raw lists', () => {
    const code = referenceFor('reverse-linked-list', goldens);
    const remote = runResponseOf(judge.runBackend({ ...payloadFor('reverse-linked-list', code), paramTypes: undefined, returnType: undefined }), 'backend-run');
    expect(remote.total_correct).toBe(0);
    expect(runResponseOf(judge.run(payloadFor('reverse-linked-list', code)), 'offline').status_msg).toBe('Accepted');
  });

  it('given the standard group-anagrams solution, when it runs, then it fails one case because the catalog expects a particular outer order the setOfSorted mode compares', () => {
    // Pinned catalog quirk: the backend scores the same 10 of 11, so the local verdict agrees with the remote one.
    const local = runResponseOf(judge.run(payloadFor('group-anagrams', named.GROUP_ANAGRAMS ?? '')), 'offline');
    const remote = runResponseOf(judge.runBackend(payloadFor('group-anagrams', named.GROUP_ANAGRAMS ?? '')), 'backend-run');
    expect([local.total_correct, local.total_testcases]).toEqual([10, 11]);
    expect([remote.total_correct, remote.total_testcases]).toEqual([10, 11]);
  });
});
