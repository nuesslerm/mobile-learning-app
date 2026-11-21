import { describe, expect, it } from 'vitest';

import { ProblemCatalog } from '../../../domain/problemCatalog';
import { runParamsFor } from '../../../domain/run/runParams';
import { answers, createFakePageHost, HANG, type FakePageHost, type FakePageScript } from '../../../testing/fakePageHost';
import { createPyodideRunner, type RunnerLimits, type RunnerState } from '../pyodideRunner';

const problem = ProblemCatalog.getById('two-sum');
if (!problem) throw new Error('the catalog has no two-sum');
const params = (code = 'code') => runParamsFor(problem, 'python3', code);
const FAST: Partial<RunnerLimits> = { readyMs: 1000, pullMs: 1000, bootMs: 1000, waitMs: 1000, runMs: 100, rpcSlackMs: 60, pingMs: 200, idleCheckMs: 60_000 };
const settled = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function setup(script: FakePageScript = {}, limits: Partial<RunnerLimits> = {}) {
  const host: FakePageHost = createFakePageHost(script);
  const runner = createPyodideRunner({ host, limits: { ...FAST, ...limits } });
  return { host, runner };
}

describe('the Pyodide runner', () => {
  it('Given a page that is not loaded yet, When a run is made, Then it waits for the load and answers Accepted over every case', async () => {
    const { host, runner } = setup({ readyAfterMs: 20 });
    const response = await runner.run(params());
    expect(response).toMatchObject({ source: 'offline', status_msg: 'Accepted', total_correct: 12, total_testcases: 12 });
    expect(host.loads).toBe(1);
  });

  it('Given two runs made together, When they execute, Then they run one at a time in order on one page', async () => {
    let active = 0;
    let overlapped = false;
    const order: string[] = [];
    const { host, runner } = setup({
      judge: async (call) => {
        active += 1;
        overlapped ||= active > 1;
        order.push(call.payload.code);
        await settled(15);
        active -= 1;
        return answers.allPass(call);
      },
    });
    await Promise.all([runner.run(params('first')), runner.run(params('second'))]);
    expect([overlapped, order, host.loads]).toEqual([false, ['first', 'second'], 1]);
  });

  it('Given the interpreter fails to boot, When a run is made, Then it fails as unavailable at the boot stage, and the next run loads again and passes', async () => {
    const { host, runner } = setup({ bootFails: 'out of memory' });
    await expect(runner.run(params())).rejects.toMatchObject({ name: 'CodeRunnerError', kind: 'unavailable', stage: 'boot' });
    expect(runner.state).toBe('failed');
    host.script({ bootFails: undefined });
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Accepted' });
    expect(host.loads).toBe(2);
  });

  it('Given the page cannot read the runtime files, When it loads, Then the files are pushed in pieces and the run passes', async () => {
    const { host, runner } = setup({ pullFails: true });
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Accepted' });
    expect(host.calls.filter((c) => c.method === 'chunk')).toHaveLength(8);
  });

  it('Given the assets cannot be resolved, When a run is made, Then it fails as unavailable at the assets stage', async () => {
    const { runner } = setup({ assetsFail: 'the file is gone' });
    await expect(runner.run(params())).rejects.toMatchObject({ kind: 'unavailable', stage: 'assets', message: 'the file is gone' });
  });

  it('Given a page that never answers, When the whole-run limit passes, Then the verdict is a timeout, the page is thrown away and a fresh one answers the next run', async () => {
    const { host, runner } = setup({ judge: (call, load) => (load === 1 ? HANG : answers.allPass(call)) });
    const response = await runner.run(params());
    expect(response).toMatchObject({ state: 'FAILURE', status_msg: 'Error', runtime_error: 'Timeout: execution exceeded process limit', total_testcases: 12 });
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Accepted' });
    expect([host.loads, host.closes]).toEqual([2, 1]);
  });

  it('Given the page dies while running, When it is reported lost, Then the verdict says the runtime stopped and the next run still works', async () => {
    const { host, runner } = setup({ judge: (call, load) => (load === 1 ? HANG : answers.allPass(call)) });
    const running = runner.run(params());
    await settled(40);
    host.crash('killed');
    expect(await running).toMatchObject({ status_msg: 'Error', runtime_error: expect.stringContaining('stopped') });
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Accepted' });
    expect(host.loads).toBe(2);
  });

  it('Given the interpreter hits a fatal stack error, When the run answers, Then the verdict names deep recursion and a fresh page is loaded', async () => {
    const { host, runner } = setup({
      judge: (call, load) => {
        if (load === 1) throw new Error('RangeError: Maximum call stack size exceeded');
        return answers.allPass(call);
      },
    });
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Error', runtime_error: expect.stringContaining('recursion') });
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Accepted' });
    expect(host.loads).toBe(2);
  });

  it('Given the page dies while idle, When the next run is made, Then no reload was started and the run loads a fresh page', async () => {
    const { host, runner } = setup();
    await runner.run(params());
    host.crash('process gone');
    await settled(20);
    expect([runner.state, host.loads]).toEqual(['idle', 1]);
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Accepted' });
    expect(host.loads).toBe(2);
  });

  it('Given a load that is slower than a run will wait, When the run gives up, Then it fails at the wait stage while the load carries on and serves the next run', async () => {
    const { host, runner } = setup({ bootMs: 150 }, { waitMs: 30 });
    await expect(runner.run(params())).rejects.toMatchObject({ kind: 'unavailable', stage: 'wait' });
    await settled(200);
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Accepted' });
    expect(host.loads).toBe(1);
  });

  it('Given queued runs, When the runner is disposed, Then they fail as closed, and a later run works', async () => {
    const { runner } = setup({ readyAfterMs: 30 });
    const first = runner.run(params());
    const second = runner.run(params());
    const failures = Promise.allSettled([first, second]);
    runner.dispose();
    expect((await failures).map((r) => r.status === 'rejected' && (r.reason as { kind: string }).kind)).toEqual(['closed', 'closed']);
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Accepted' });
  });

  it('Given an answer that is not a harness report, When the run answers, Then it fails as closed and the next run still passes', async () => {
    const { runner } = setup({ judge: (call, load) => (load === 1 ? ({ ok: true, results: [{ nonsense: 1 }] } as never) : answers.allPass(call)) });
    await expect(runner.run(params())).rejects.toMatchObject({ kind: 'closed' });
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Accepted' });
  });

  it('Given a case that raises, When the run answers, Then the verdict stays Wrong Answer and carries the error and its line', async () => {
    const { runner } = setup({ judge: (call) => answers.caseError(call, 'IndexError: list index out of range', { line: 4, column: 9 }, 2) });
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Wrong Answer', total_correct: 11, runtime_error: 'IndexError: list index out of range', error_at: { line: 4, column: 9 } });
  });

  it('Given a syntax error, When the run answers, Then it is a compile error with the line', async () => {
    const { runner } = setup({ judge: () => answers.compileError("CompileError: expected ':'", { line: 2 }) });
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Error', compile_error: "CompileError: expected ':'", error_at: { line: 2 } });
  });

  it('Given a language that is not Python, When a run is made, Then it answers Unsupported language without loading a page', async () => {
    const { host, runner } = setup();
    expect(await runner.run({ ...params(), lang: 'javascript' })).toMatchObject({ status_msg: 'Error', compile_error: 'Unsupported language: javascript' });
    expect(host.loads).toBe(0);
  });

  it('Given a page idle longer than the check interval, When a run is made, Then the page is pinged before it is trusted', async () => {
    const { host, runner } = setup({}, { idleCheckMs: 20 });
    await runner.run(params());
    await settled(40);
    await runner.run(params());
    expect(host.calls.filter((c) => c.method === 'ping')).toHaveLength(1);
  });

  it('Given a run, When it goes from idle to answered, Then the states are loading, ready, running, ready', async () => {
    const { runner } = setup();
    const seen: RunnerState[] = [];
    runner.subscribe((s) => seen.push(s));
    await runner.run(params());
    expect(seen).toEqual(['loading', 'ready', 'running', 'ready']);
  });

  it('Given a run is asked to carry the problem’s value types, When it reaches the page, Then paramTypes, returnType and inPlace arrive with the limits', async () => {
    const node = ProblemCatalog.getById('reverse-linked-list');
    if (!node) throw new Error('no reverse-linked-list');
    const { host, runner } = setup();
    await runner.run(runParamsFor(node, 'python3', 'c'));
    const judge = host.calls.find((c) => c.method === 'judge')?.params as { payload: Record<string, unknown> };
    expect(judge.payload).toMatchObject({ paramTypes: node.paramTypes, returnType: node.returnType, inPlace: null, timeoutMs: 5000, processLimitMs: 100 });
  });

  it('Given a run with a harness, When it is sent to the page, Then the judge call’s payload carries it, and a run without one carries none', async () => {
    const { host, runner } = setup();
    await runner.run({ ...params(), harness: { kind: 'design', className: 'Trie' } });
    await runner.run(params());
    const payloads = host.calls.filter((c) => c.method === 'judge').map((c) => (c.params as { payload: Record<string, unknown> }).payload);
    expect(payloads[0]).toMatchObject({ harness: { kind: 'design', className: 'Trie' } });
    expect(payloads[1]?.harness).toBeFalsy();
  });
});
