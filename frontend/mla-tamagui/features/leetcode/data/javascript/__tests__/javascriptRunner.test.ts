import { describe, expect, it } from 'vitest';

import { ProblemCatalog } from '../../../domain/problemCatalog';
import { runParamsFor } from '../../../domain/run/runParams';
import { answers, createFakePageHost, HANG, type FakePageScript } from '../../../testing/fakePageHost';
import type { RunnerLimits, RunnerState } from '../../runner/pageRunner';
import { createJavaScriptRunner } from '../javascriptRunner';
import { signal } from './support/signal';

const problem = ProblemCatalog.getById('two-sum');
if (!problem) throw new Error('the catalog has no two-sum');
const params = (code = 'code') => runParamsFor(problem, 'javascript', code);
const FAST: Partial<RunnerLimits> = { readyMs: 1000, bootMs: 1000, waitMs: 1000, runMs: 100, rpcSlackMs: 60, pingMs: 200, idleCheckMs: 60_000 };

/** A JavaScript page: no runtime files, so the session lists none. */
function setup(script: FakePageScript = {}) {
  const host = createFakePageHost({ files: {}, ...script });
  return { host, runner: createJavaScriptRunner({ host, limits: FAST }) };
}

describe('the JavaScript runner', () => {
  it('Given a page that lists no files, When a run is made, Then it answers Accepted over every case and neither pulls nor pushes a file', async () => {
    const { host, runner } = setup();
    expect(await runner.run(params())).toMatchObject({ source: 'offline', status_msg: 'Accepted', total_correct: 12, total_testcases: 12 });
    const methods = host.calls.map((call) => call.method);
    expect(methods.filter((method) => method === 'boot')).toHaveLength(1);
    expect(methods).not.toContain('pull');
    expect(methods).not.toContain('chunk');
    expect(host.loads).toBe(1);
  });

  it('Given a page that cannot start a worker, When a run is made, Then it fails as unavailable at the boot stage, and the next run loads again and passes', async () => {
    const { host, runner } = setup({ bootFails: 'cannot start a worker (blob: Worker is not defined; data: Worker is not defined)' });
    await expect(runner.run(params())).rejects.toMatchObject({ name: 'CodeRunnerError', kind: 'unavailable', stage: 'boot', message: expect.stringContaining('cannot start a worker') });
    expect(runner.state).toBe('failed');
    host.script({ bootFails: undefined });
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Accepted' });
    expect(host.loads).toBe(2);
  });

  it('Given a Python run, When it is made, Then the answer is Unsupported language: python3 and no page is loaded', async () => {
    const { host, runner } = setup();
    expect(await runner.run({ ...params(), lang: 'python3' })).toMatchObject({ status_msg: 'Error', compile_error: 'Unsupported language: python3' });
    expect(host.loads).toBe(0);
  });

  it('Given a page that never answers, When the whole-run limit passes, Then the verdict is a timeout, the page is thrown away and a fresh one answers the next run', async () => {
    const { host, runner } = setup({ judge: (call, load) => (load === 1 ? HANG : answers.allPass(call)) });
    expect(await runner.run(params())).toMatchObject({ state: 'FAILURE', status_msg: 'Error', runtime_error: 'Timeout: execution exceeded process limit', total_testcases: 12 });
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Accepted' });
    expect([host.loads, host.closes]).toEqual([2, 1]);
  });

  it('Given the page is lost while a run is in flight, When it is reported lost, Then the verdict says the JavaScript runtime stopped and the next run loads a fresh page', async () => {
    const inFlight = signal<number>();
    const { host, runner } = setup({
      judge: (call, load) => {
        if (load !== 1) return answers.allPass(call);
        inFlight.push(load);
        return HANG;
      },
    });
    const running = runner.run(params());
    await inFlight.next();
    host.crash('killed');
    expect(await running).toMatchObject({ status_msg: 'Error', runtime_error: 'The JavaScript runtime stopped while running your code (it may have run out of memory).' });
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Accepted' });
    expect(host.loads).toBe(2);
  });

  it('Given the page reports a stack overflow of its own, When the run answers, Then the verdict says the JavaScript runtime crashed and a fresh page is loaded', async () => {
    const { host, runner } = setup({
      judge: (call, load) => {
        if (load === 1) throw new Error('RangeError: Maximum call stack size exceeded');
        return answers.allPass(call);
      },
    });
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Error', runtime_error: 'The JavaScript runtime crashed.' });
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Accepted' });
    expect(host.loads).toBe(2);
  });

  it('Given a case that raises, When the run answers, Then the verdict carries the error and its line as the Python runner’s does', async () => {
    const { runner } = setup({ judge: (call) => answers.caseError(call, "TypeError: Cannot read properties of undefined (reading 'length')", { line: 4, column: 9 }, 2) });
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Wrong Answer', total_correct: 11, runtime_error: "TypeError: Cannot read properties of undefined (reading 'length')", error_at: { line: 4, column: 9 } });
  });

  it('Given a syntax error, When the run answers, Then it is a compile error with the line', async () => {
    const { runner } = setup({ judge: () => answers.compileError("CompileError: SyntaxError: Unexpected token ';'", { line: 2 }) });
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Error', compile_error: "CompileError: SyntaxError: Unexpected token ';'", error_at: { line: 2 } });
  });

  it('Given a run, When it goes from idle to answered, Then the states are loading, ready, running, ready', async () => {
    const { runner } = setup();
    const seen: RunnerState[] = [];
    runner.subscribe((state) => seen.push(state));
    await runner.run(params());
    expect(seen).toEqual(['loading', 'ready', 'running', 'ready']);
  });

  it('Given a warm-up and no run, When the page has loaded, Then it is ready and a run reuses the page', async () => {
    const { host, runner } = setup();
    const ready = signal<RunnerState>();
    runner.subscribe((state) => state === 'ready' && ready.push(state));
    runner.warmUp('javascript');
    await ready.next();
    expect(await runner.run(params())).toMatchObject({ status_msg: 'Accepted' });
    expect(host.loads).toBe(1);
  });

  it('Given a problem with value types, When it is run, Then the judge call carries them with the limits, as it does for Python', async () => {
    const node = ProblemCatalog.getById('reverse-linked-list');
    if (!node) throw new Error('no reverse-linked-list');
    const { host, runner } = setup();
    await runner.run(runParamsFor(node, 'javascript', 'c'));
    const judge = host.calls.find((call) => call.method === 'judge')?.params as { payload: Record<string, unknown> };
    expect(judge.payload).toMatchObject({ paramTypes: node.paramTypes, returnType: node.returnType, inPlace: null, timeoutMs: 5000, processLimitMs: 100 });
  });
});
