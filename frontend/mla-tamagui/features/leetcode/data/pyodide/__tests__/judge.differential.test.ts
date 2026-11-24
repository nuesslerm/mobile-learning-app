import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { beforeAll, describe, expect, it, vi } from 'vitest';

import type { TestCase, TestResult } from '../../../domain/run/execution';
import { ProblemCatalog } from '../../../domain/problemCatalog';
import { runResponseOf, type RawRun } from '../../../domain/run/verdict';
import type { JudgePayload } from '../../runner/judgeProtocol';
import { bootNodeJudge, type NodeJudge } from './support/nodePyodide';

vi.setConfig({ testTimeout: 60_000, hookTimeout: 120_000 });

type Scenario = { name: string; kind: 'parity' | 'diverge'; problem: string; code: string; timeoutMs?: number; cases?: number; rawLists?: boolean; inPlace?: string };
const scenarios = JSON.parse(readFileSync(join(__dirname, 'fixtures/scenarios.json'), 'utf8')) as Scenario[];

const payloadOf = (s: Scenario, catalogTypes: boolean): JudgePayload => {
  const problem = ProblemCatalog.getById(s.problem);
  if (!problem) throw new Error(`no problem ${s.problem}`);
  const two = ProblemCatalog.getById('two-sum')?.testCases ?? [];
  const cases: TestCase[] = s.cases === undefined ? (problem.testCases ?? []) : two.slice(0, s.cases);
  return {
    code: s.code,
    methodName: problem.methodName ?? '',
    testCases: cases,
    timeoutMs: s.timeoutMs ?? 5000,
    ...(catalogTypes && !s.rawLists ? { paramTypes: problem.paramTypes, returnType: problem.returnType, inPlace: s.inPlace ?? null } : {}),
  };
};

// what must be equal wherever the backend works: pass, the value, the error's type, both streams
const keyOf = (r: TestResult) => [r.pass, r.error ? null : JSON.stringify(r.actual), r.error ? r.error.split(':')[0] : null, r.stdout, r.stderr];
const verdictOf = (raw: RawRun) => {
  const { error_at: _location, ...plain } = raw; // the local run lifts the line out of the message: same text otherwise
  const response = runResponseOf(plain, 'offline');
  return [response.status_msg, response.compile_error ?? null, response.runtime_error ?? null, response.total_correct, response.total_testcases];
};
// set iteration order and an object's address differ between interpreters for reasons outside the harness
const OUTSIDE_THE_HARNESS = new Set(['3sum set-based (setOfSorted)', 'returns custom object']);

describe('the on-device judge against the backend script', () => {
  let judge: NodeJudge;
  beforeAll(async () => {
    judge = await bootNodeJudge();
  });

  it('given the backend python.py and judge.py in one interpreter, when the parity scenarios run, then every row and the whole-run verdict agree', () => {
    // Given the scenarios the backend itself can run (its process runner sends no paramTypes)
    const compared = scenarios.filter((s) => s.kind === 'parity' && !OUTSIDE_THE_HARNESS.has(s.name) && s.timeoutMs === undefined && !s.name.includes('recursion'));
    expect(compared.length).toBeGreaterThanOrEqual(20);

    for (const scenario of compared) {
      // When both run the same payload
      const payload = payloadOf(scenario, false);
      const local = judge.run(payload);
      const remote = judge.runBackend(payload);

      // Then rows, errors and the verdict are equal
      expect(local.results?.map(keyOf), scenario.name).toEqual(remote.results?.map(keyOf));
      expect(verdictOf(local), scenario.name).toEqual(verdictOf(remote));
      expect(local.error, scenario.name).toBe(remote.error);
    }
  });

  it('given the backend script is copied into the fixtures, when it is compared with the script the backend ships, then they are the same bytes', () => {
    const shipped = readFileSync(join(__dirname, '../../../../../../../backend/leetcode-backend/rs/harness/python.py'));
    expect(readFileSync(join(__dirname, 'fixtures/backend-python-harness.py')).equals(shipped)).toBe(true);
  });
});
