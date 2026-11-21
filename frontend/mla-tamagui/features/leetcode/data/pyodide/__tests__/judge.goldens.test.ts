import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { beforeAll, describe, expect, it, vi } from 'vitest';

import { ProblemCatalog } from '../../../domain/problemCatalog';
import { runParamsFor } from '../../../domain/run/runParams';
import { runResponseOf } from '../../../domain/run/verdict';
import type { JudgePayload } from '../judgeProtocol';
import { bootNodeJudge, type NodeJudge } from './support/nodePyodide';
import { loadReferenceCorpus, referenceFor } from './support/referenceCorpus';

vi.setConfig({ testTimeout: 60_000, hookTimeout: 120_000 });

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

  it('given the reference corpus of at least the 29 problems across the patterns, when it runs with the catalog paramTypes, returnType and inPlaceParam, then every case passes', () => {
    // Given the 29 problems the corpus began with: arrays, two pointers, stack, binary search, DP, intervals, graphs, linked lists, trees, in-place matrices
    expect(goldens.length).toBeGreaterThanOrEqual(29);
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
    expect(cases).toBeGreaterThan(300);
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
