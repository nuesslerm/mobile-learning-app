import { describe, expect, it, vi } from 'vitest';

import type { TestCase } from '../../../domain/run/execution';
import type { JudgePayload } from '../../runner/judgeProtocol';
import { runBackendHarness } from './support/backendNodeHarness';
import { judge } from './support/judgeRun';

vi.setConfig({ testTimeout: 60_000 });

type Scenario = { name: string; code: string; methodName: string; cases: TestCase[] };

const scenarios: Scenario[] = [
  {
    name: 'an exact answer that matches',
    code: 'class Solution {\n  add(a, b) { return a + b; }\n}',
    methodName: 'add',
    cases: [{ inputs: { a: 1, b: 2 }, expected: 3 }],
  },
  {
    name: 'an exact answer that is wrong',
    code: 'class Solution {\n  add(a, b) { return a + b; }\n}',
    methodName: 'add',
    cases: [{ inputs: { a: 1, b: 2 }, expected: 4 }],
  },
  {
    name: 'a list answered as the same items in another order, compared as sorted',
    code: 'class Solution {\n  pair() { return [1, 0]; }\n}',
    methodName: 'pair',
    cases: [{ inputs: {}, expected: [0, 1], compareMode: 'sorted' }],
  },
  {
    name: 'a sorted comparison that is wrong',
    code: 'class Solution {\n  pair() { return [1, 2]; }\n}',
    methodName: 'pair',
    cases: [{ inputs: {}, expected: [0, 1], compareMode: 'sorted' }],
  },
  {
    name: 'lists of lists with inner items in another order, compared as setOfSorted',
    code: 'class Solution {\n  groups() { return [[2, 1], [3]]; }\n}',
    methodName: 'groups',
    cases: [{ inputs: {}, expected: [[1, 2], [3]], compareMode: 'setOfSorted' }],
  },
  {
    name: 'a setOfSorted comparison with the outer order changed',
    code: 'class Solution {\n  groups() { return [[3], [2, 1]]; }\n}',
    methodName: 'groups',
    cases: [{ inputs: {}, expected: [[1, 2], [3]], compareMode: 'setOfSorted' }],
  },
  {
    name: 'a TypeError thrown by the solution',
    code: 'class Solution {\n  boom() { throw new TypeError("bad"); }\n}',
    methodName: 'boom',
    cases: [{ inputs: {}, expected: 1 }],
  },
  {
    name: 'a null dereference',
    code: 'class Solution {\n  boom(x) { return x.y.z; }\n}',
    methodName: 'boom',
    cases: [{ inputs: { x: {} }, expected: 1 }],
  },
  {
    name: 'a string answer',
    code: 'class Solution {\n  word(n) { return "w".repeat(n); }\n}',
    methodName: 'word',
    cases: [{ inputs: { n: 3 }, expected: 'www' }, { inputs: { n: 2 }, expected: 'w' }],
  },
  {
    name: 'a boolean answer against a number',
    code: 'class Solution {\n  yes() { return true; }\n}',
    methodName: 'yes',
    cases: [{ inputs: {}, expected: 1 }],
  },
];

const errorName = (error: string | null | undefined) => (error ? error.split(':')[0] : null);

describe('the backend’s Node harness and the on-device harness agree on a class Solution', () => {
  it.each(scenarios)('Given $name, When both harnesses run it, Then pass, the JSON-clean actual and the error name agree', async ({ code, methodName, cases }) => {
    const payload: JudgePayload = { code, methodName, testCases: cases };
    const remote = await runBackendHarness(payload);
    const { raw } = await judge(payload);

    expect(remote.ok).toBe(true);
    expect(raw.ok).toBe(true);
    const remoteRows = remote.results ?? [];
    const localRows = raw.results ?? [];
    expect(localRows.map((row) => row.pass)).toEqual(remoteRows.map((row) => row.pass));
    expect(localRows.map((row) => JSON.parse(JSON.stringify(row.actual ?? null)))).toEqual(remoteRows.map((row) => JSON.parse(JSON.stringify(row.actual ?? null))));
    expect(localRows.map((row) => errorName(row.error))).toEqual(remoteRows.map((row) => errorName(row.error)));
  });
});
