import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it, vi } from 'vitest';

import { ProblemCatalog } from '../../../domain/problemCatalog';
import { runParamsFor } from '../../../domain/run/runParams';
import { runResponseOf } from '../../../domain/run/verdict';
import type { JudgePayload } from '../../runner/judgeProtocol';
import { judge } from './support/judgeRun';
import { loadReferenceCorpus } from './support/referenceCorpus';

vi.setConfig({ testTimeout: 120_000 });

/** group-anagrams: the standard solution fails one case whose outer order the catalog fixed, so the named test below pins it instead of a reference. */
const NO_REFERENCE = ['group-anagrams'];

const goldens = loadReferenceCorpus();
const named = JSON.parse(readFileSync(join(__dirname, 'fixtures/named.json'), 'utf8')) as Record<string, string>;

function payloadFor(problemId: string, code: string): JudgePayload {
  const { lang: _lang, ...payload } = runParamsFor(ProblemCatalog.getByIdOrThrow(problemId), 'javascript', code);
  return payload;
}

describe('the on-device JavaScript judge on known-good solutions', () => {
  it('Given the catalog, When the corpus is listed, Then every problem has a reference and the only exception is the one whose standard solution fails a pinned case', () => {
    const covered = new Set(goldens.map(({ problem }) => problem));

    // The failure lists every catalog problem that has no known-good solution
    expect(ProblemCatalog.getAll().map((problem) => problem.id).filter((id) => !covered.has(id))).toEqual(NO_REFERENCE);
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

  it('Given the standard group-anagrams solution, When it runs, Then it fails one case because the catalog fixes an outer order that the setOfSorted mode compares', async () => {
    // Pinned catalog quirk: the Python goldens pin the same 10 of 11, and the backend cannot score a JavaScript starter, so only the local verdict is read
    const { raw } = await judge(payloadFor('group-anagrams', named.GROUP_ANAGRAMS ?? ''));
    const response = runResponseOf(raw, 'offline');

    expect([response.total_correct, response.total_testcases]).toEqual([10, 11]);
  });
});
