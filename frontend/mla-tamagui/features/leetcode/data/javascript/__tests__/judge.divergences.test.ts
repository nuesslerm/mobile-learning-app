import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it, vi } from 'vitest';

import { ProblemCatalog } from '../../../domain/problemCatalog';
import type { TestCase } from '../../../domain/run/execution';
import type { JudgePayload } from '../../runner/judgeProtocol';
import { backendHarnessSource, runBackendHarness } from './support/backendNodeHarness';
import { judge, startJudge, watched } from './support/judgeRun';
import { spawnNodeWorker } from './support/nodeWorkerSpawn';
import { signal } from './support/signal';

vi.setConfig({ testTimeout: 60_000 });

const named = JSON.parse(readFileSync(join(__dirname, 'fixtures/named.json'), 'utf8')) as Record<string, string>;
const source = (key: string): string => named[key] ?? '';
const twoSumCases = (n: number): TestCase[] => (ProblemCatalog.getById('two-sum')?.testCases ?? []).slice(0, n);
const twoSum = (code: string, cases = 3, extra: Partial<JudgePayload> = {}): JudgePayload => ({ code, methodName: 'twoSum', testCases: twoSumCases(cases), ...extra });

/**
 * Where the backend’s Node harness breaks or cannot run, and the device’s harness does the sensible thing, and the places the device
 * differs from LeetCode’s Node on purpose. Each is pinned on both sides where the backend can run it, so a change is a decision.
 */
describe('the deliberate differences from the backend', () => {
  it('Given a console.log at module level, When it runs, Then the backend loses the whole run and the device puts the text in the first case’s stdout', async () => {
    expect((await runBackendHarness(twoSum(source('MODULE_LOG')))).error).toBe('Runner produced no parseable output');
    const { raw } = await judge(twoSum(source('MODULE_LOG')));
    expect(raw.results?.[0]?.stdout).toBe('module level\n');
    expect(raw.results?.[1]?.stdout).toBe('');
  });

  it('Given a console.log inside the method, When it runs, Then the backend loses the run and the device gives each case its own stdout', async () => {
    expect((await runBackendHarness(twoSum(source('CASE_LOG')))).error).toBe('Runner produced no parseable output');
    const { raw } = await judge(twoSum(source('CASE_LOG')));
    expect(raw.results?.map((row) => row.stdout)).toEqual(twoSumCases(3).map((testCase) => `case ${(testCase.inputs.nums as number[]).length}\n`));
  });

  it('Given a solution that mutates its input, When it runs, Then the device shows the input the case had and the backend shows the mutated one', async () => {
    const original = twoSumCases(2)[1]?.inputs;
    expect((await judge(twoSum(source('MUTATES_INPUT'), 2))).raw.results?.[1]?.input).toEqual(original);
    expect((await runBackendHarness(twoSum(source('MUTATES_INPUT'), 2))).results?.[1]?.input).not.toEqual(original);
  });

  it('Given a solution that returns NaN, When it runs against null, Then the backend passes it (NaN prints as null) and the device fails it and shows NaN', async () => {
    const payload = twoSum(source('RETURNS_NAN'), 1, { testCases: [{ inputs: {}, expected: null }] });
    expect((await runBackendHarness(payload)).results?.[0]?.pass).toBe(true);
    expect((await judge(payload)).raw.results?.[0]).toMatchObject({ pass: false, actual: 'NaN' });
  });

  it('Given a solution that returns undefined, When it runs against null, Then the backend fails it and the device reads it as null and passes', async () => {
    const payload = twoSum(source('RETURNS_UNDEFINED'), 1, { testCases: [{ inputs: {}, expected: null }] });
    expect((await runBackendHarness(payload)).results?.[0]?.pass).toBe(false);
    expect((await judge(payload)).raw.results?.[0]).toMatchObject({ pass: true, actual: null });
  });

  it('Given an object answer whose keys come in another order than the expected one, When it runs, Then the backend fails it and the device passes it', async () => {
    const payload: JudgePayload = { code: 'class Solution {\n  obj() { return { b: 1, a: 2 }; }\n}', methodName: 'obj', testCases: [{ inputs: {}, expected: { a: 2, b: 1 } }] };
    expect((await runBackendHarness(payload)).results?.[0]?.pass).toBe(false);
    expect((await judge(payload)).raw.results?.[0]?.pass).toBe(true);
  });

  it('Given a solution written as var twoSum = function, When it runs, Then the backend refuses it and the device runs it', async () => {
    expect((await runBackendHarness(twoSum(source('TWO_SUM_FUNCTION')))).error).toBe('No Solution class defined');
    const { raw } = await judge(twoSum(source('TWO_SUM_FUNCTION'), 12));
    expect(raw.results?.map((row) => row.pass)).toEqual(twoSumCases(12).map(() => true));
  });

  it('Given the class form of two-sum, When both harnesses run it on every bundled case, Then both accept all 12', async () => {
    const payload = twoSum(source('TWO_SUM_CLASS'), 12);
    const remote = await runBackendHarness(payload);
    const { raw } = await judge(payload);
    expect(remote.results?.every((row) => row.pass)).toBe(true);
    expect(raw.results?.every((row) => row.pass)).toBe(true);
  });

  it('Given a harness-only key such as pos, When it runs, Then the device passes one argument and the backend passes it as a second', async () => {
    const payload: JudgePayload = {
      code: source('HARNESS_ONLY_KEY'),
      methodName: 'hasCycle',
      testCases: [{ inputs: { head: [1, 2], pos: 0 }, expected: 1 }],
      paramTypes: { head: 'ListNode', pos: 'ListNode_cycle_pos' },
    };
    expect((await judge(payload)).raw.results?.[0]?.pass).toBe(true);
    expect((await runBackendHarness(payload)).results?.[0]?.pass).toBe(false);
  });

  it('Given a case that never ends, When the backend’s source is read, Then it has no limit at all, and the device ends the case at its deadline', async () => {
    expect(backendHarnessSource()).not.toMatch(/timeout|deadline|terminate|setTimeout/i);
    const begins = signal<number>();
    const spawn = watched(spawnNodeWorker, { message: (message) => message.type === 'begin' && begins.push(message.index) });
    const { done, timers } = startJudge(twoSum(source('INFINITE_LOOP_ONE_CASE'), 3), { spawn });
    expect(await begins.next()).toBe(0);
    expect(await begins.next()).toBe(1);
    timers.fire(5000);
    const rows = (await done).raw.results ?? [];
    expect(rows.map((row) => row.pass)).toEqual([true, false, true]);
    expect(rows[1]?.error).toBe('TimeoutError: Code execution timed out (5s limit)');
  });

  it('Given a case that prints and then never ends, When its deadline fires, Then the output it had printed is lost: the worker is ended before it can say it', async () => {
    const begins = signal<number>();
    const spawn = watched(spawnNodeWorker, { message: (message) => message.type === 'begin' && begins.push(message.index) });
    const code = 'var twoSum = function() { console.log("before the loop"); while (true) {} };';
    const { done, timers } = startJudge(twoSum(code, 1), { spawn });
    await begins.next();
    timers.fire(5000);
    expect((await done).raw.results?.[0]).toMatchObject({ error: 'TimeoutError: Code execution timed out (5s limit)', stdout: '' });
  });

  it('Given module state and a timeout, When the next case runs, Then it runs on a new worker with the module state reset', async () => {
    const begins = signal<number>();
    const spawn = watched(spawnNodeWorker, { message: (message) => message.type === 'begin' && begins.push(message.index) });
    const cases: TestCase[] = [{ inputs: { nums: [0] }, expected: 1 }, { inputs: { nums: [3, 2, 4] }, expected: 0 }, { inputs: { nums: [0] }, expected: 1 }, { inputs: { nums: [0] }, expected: 2 }];
    const { done, timers } = startJudge({ code: source('COUNTS_CALLS'), methodName: 'twoSum', testCases: cases }, { spawn });
    expect(await begins.next()).toBe(0);
    expect(await begins.next()).toBe(1);
    timers.fire(5000);
    const rows = (await done).raw.results ?? [];
    expect(rows.map((row) => row.actual)).toEqual([1, null, 1, 2]);
  });

  it('Given a leading use strict, When the code runs, Then the directive is not honoured: this in a plain call is the global object', async () => {
    const { raw } = await judge(twoSum(source('STRICT_DIRECTIVE'), 1, { testCases: [{ inputs: {}, expected: 'object' }] }));
    expect(raw.results?.[0]).toMatchObject({ pass: true, actual: 'object' });
  });

  it('Given an async function, When it runs, Then the promise is not awaited and is shown by String (LeetCode’s judge does not await it either)', async () => {
    const { raw } = await judge(twoSum(source('ASYNC_FUNCTION'), 1, { testCases: [{ inputs: {}, expected: [0, 1] }] }));
    expect(raw.results?.[0]).toMatchObject({ pass: false, actual: '[object Promise]' });
  });

  it('Given a call to require, When it runs, Then Node’s globals are absent and the row is a ReferenceError', async () => {
    const { raw } = await judge(twoSum(source('NO_REQUIRE'), 1));
    expect(raw.results?.[0]?.error).toBe('ReferenceError: require is not defined');
  });
});
