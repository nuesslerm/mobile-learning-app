import { describe, expect, it, vi } from 'vitest';

import type { TestCase } from '../../../domain/run/execution';
import type { JudgePayload } from '../../runner/judgeProtocol';
import type { WorkerSpawn } from '../workerEngine';
import { judge, startJudge, watched } from './support/judgeRun';
import { spawnNodeWorker } from './support/nodeWorkerSpawn';
import { signal } from './support/signal';

vi.setConfig({ testTimeout: 60_000 });

const CASE_MS = 5000;
const payload = (code: string, cases: TestCase[], extra: Partial<JudgePayload> = {}): JudgePayload => ({ code, methodName: 'twoSum', testCases: cases, ...extra });
const num = (n: number): TestCase => ({ inputs: { n }, expected: n });

/** A spawn of real worker threads that tells the test when a case has begun in one, and which workers started and were ended. */
function observed() {
  const begins = signal<number>();
  const spawned = signal<string>();
  const started: string[] = [];
  const ended: string[] = [];
  const spawn = watched(spawnNodeWorker, {
    message: (message) => message.type === 'begin' && begins.push(message.index),
    spawned: (name) => {
      started.push(name);
      spawned.push(name);
    },
    ended: (name) => void ended.push(name),
  });
  return { spawn, begins, spawned, started, ended };
}

describe('code that never ends', () => {
  it('Given a case that never ends, When its deadline fires after the worker began it, Then that row is the 5s TimeoutError with no error_at and the next case passes on a new worker', async () => {
    const { spawn, begins, started } = observed();
    const { done, timers } = startJudge(payload('var twoSum = function(n) { if (n === 1) { while (true) {} } return n; };', [num(1), num(2)]), { spawn });

    expect(await begins.next()).toBe(0);
    expect(timers.fire(CASE_MS)).toBe(1);
    const { raw, emitted } = await done;

    expect(raw.ok).toBe(true);
    expect(raw.results?.[0]).toMatchObject({ pass: false, error: 'TimeoutError: Code execution timed out (5s limit)', input: { n: 1 } });
    expect(raw.results?.[0]?.error_at).toBeUndefined();
    expect(raw.results?.[1]).toMatchObject({ pass: true, actual: 2 });
    expect(started).toHaveLength(2);
    expect(emitted.map((row) => row.index)).toEqual([0, 1]);
  });

  it('Given a loop that swallows errors inside a try, When the deadline fires, Then it ends the same way: the worker is terminated, not interrupted', async () => {
    const { spawn, begins } = observed();
    const code = 'var twoSum = function(n) { while (true) { try { while (true) {} } catch (e) {} } };';
    const { done, timers } = startJudge(payload(code, [num(1)]), { spawn });
    await begins.next();
    timers.fire(CASE_MS);
    expect((await done).raw.results?.[0]?.error).toBe('TimeoutError: Code execution timed out (5s limit)');
  });

  it('Given a loop at module level, When the deadline fires, Then the run fails as kind timeout with the module-level message and no error_at', async () => {
    const { spawn, spawned } = observed();
    const { done, timers } = startJudge(payload('while (true) {}\nvar twoSum = function() { return 1; };', [num(1)]), { spawn });
    await spawned.next();
    timers.fire(CASE_MS);
    const { raw, emitted } = await done;
    expect(raw).toMatchObject({ ok: false, error_kind: 'timeout', error: 'Timeout: module-level code exceeded the 5s limit', results: [] });
    expect(raw.error_at).toBeUndefined();
    expect(emitted).toEqual([]);
  });

  it('Given a case limit of 1200 ms, When a case never ends, Then the limit is rounded up to whole seconds as the judge does', async () => {
    const { spawn, begins } = observed();
    const { done, timers } = startJudge(payload('var twoSum = function() { while (true) {} };', [num(1)], { timeoutMs: 1200 }), { spawn });
    await begins.next();
    expect(timers.fire(2000)).toBe(1);
    expect((await done).raw.results?.[0]?.error).toBe('TimeoutError: Code execution timed out (2s limit)');
  });

  it('Given cases that never end and a whole-run limit, When it is passed during the second, Then the run stops with the process-limit message and each finished row was emitted once, in order', async () => {
    const { spawn, begins } = observed();
    const code = 'var twoSum = function(n) { if (n > 0) { while (true) {} } return n; };';
    const { done, timers } = startJudge(payload(code, [num(0), num(1), num(2)], { processLimitMs: 12_000 }), { spawn });

    expect(await begins.next()).toBe(0);
    expect(await begins.next()).toBe(1);
    timers.fire(12_000);
    timers.fire(CASE_MS);
    const { raw, emitted } = await done;

    expect(raw).toMatchObject({ ok: false, error_kind: 'timeout', error: 'Timeout: execution exceeded process limit', results: [] });
    expect(emitted.map((row) => row.index)).toEqual([0, 1]);
    expect(emitted[0]?.result.pass).toBe(true);
    expect(emitted[1]?.result.error).toBe('TimeoutError: Code execution timed out (5s limit)');
  });

  it('Given a finished run that ended in a timeout, When it answers, Then every worker it started was ended and no timer is left', async () => {
    const { spawn, begins, started, ended } = observed();
    const { done, timers } = startJudge(payload('var twoSum = function(n) { if (n === 1) { while (true) {} } return n; };', [num(1), num(2)]), { spawn });
    await begins.next();
    timers.fire(CASE_MS);
    await done;
    expect([...ended].sort()).toEqual([...started].sort());
    expect(timers.pending).toEqual([]);
  });
});

describe('output and values the harness must survive', () => {
  it('Given 200,000 printed lines, When the case runs, Then its stream is capped at 256 KiB with the marker and the run is a normal one', async () => {
    const code = 'var twoSum = function() { for (let i = 0; i < 200000; i += 1) console.log("line", i); return 1; };';
    const { raw } = await judge(payload(code, [num(1)]));
    const stdout = raw.results?.[0]?.stdout ?? '';
    expect(raw.ok).toBe(true);
    expect(stdout.length).toBe(256 * 1024 + '\n...[output truncated]'.length);
    expect(stdout.endsWith('\n...[output truncated]')).toBe(true);
  });

  it('Given an error whose own printing throws, When the case runs, Then the row is a row and the next case runs', async () => {
    const code = 'var twoSum = function(n) { if (n === 1) { throw { toString() { throw new Error("no"); } }; } return n; };';
    const { raw } = await judge(payload(code, [num(1), num(2)]));
    expect(raw.results?.map((row) => [row.pass, row.error])).toEqual([[false, '<unprintable>'], [true, null]]);
  });

  it('Given answers that alone exceed the size limit, When the run ends, Then the run is a runtime failure naming the limit', async () => {
    const code = 'var twoSum = function() { return "x".repeat(3 * 1024 * 1024); };';
    const { raw } = await judge(payload(code, [num(1), num(2)]));
    expect(raw).toMatchObject({ ok: false, error_kind: 'runtime', error: 'Runner output exceeded the size limit' });
  });

  it('Given a run with no cases at all, When it runs, Then it is an ok run with no rows', async () => {
    const { raw } = await judge(payload('var twoSum = function() { return 1; };', []));
    expect(raw).toEqual({ ok: true, results: [] });
  });
});

describe('a worker that cannot run', () => {
  it('Given a host that cannot start a worker, When a run is made, Then it is a runtime failure and not a hang', async () => {
    const spawn: WorkerSpawn = () => {
      throw new Error('Worker is not defined');
    };
    const { raw } = await judge(payload('var twoSum = function() { return 1; };', [num(1)]), { spawn });
    expect(raw).toMatchObject({ ok: false, error_kind: 'runtime', error: 'Runner error: cannot start a worker: Worker is not defined' });
  });

  it('Given a host that starts one worker and no more, When a case ends it by timeout, Then the run is a runtime failure naming why and the row before it was emitted', async () => {
    const { spawn: inner, begins } = observed();
    let spawns = 0;
    const spawn: WorkerSpawn = (script, sink) => {
      spawns += 1;
      if (spawns > 1) throw new Error('out of workers');
      return inner(script, sink);
    };
    const { done, timers } = startJudge(payload('var twoSum = function(n) { while (true) {} };', [num(1), num(2)]), { spawn });
    await begins.next();
    timers.fire(CASE_MS);
    const { raw, emitted } = await done;
    expect(raw).toMatchObject({ ok: false, error_kind: 'runtime', error: 'Runner error: cannot start a worker: out of workers' });
    expect(emitted.map((row) => row.index)).toEqual([0]);
  });

  it('Given a worker that raises an uncaught error while a case is in flight, When the run is made, Then that case’s row carries the error and the next case runs on a new worker', async () => {
    let posts = 0;
    const spawn: WorkerSpawn = (script, sink) => {
      const worker = spawnNodeWorker(script, sink);
      return {
        scriptName: worker.scriptName,
        post(message) {
          posts += 1;
          if (posts === 2) sink.error({ message: 'Error: thrown out of a timer', line: 1, column: 80 });
          else worker.post(message);
        },
        terminate: () => void worker.terminate(),
      };
    };
    const { raw, emitted } = await judge(payload('var twoSum = function(n) { return n; };', [num(1), num(2), num(3)]), { spawn });
    expect(raw.ok).toBe(true);
    expect(raw.results?.map((row) => [row.pass, row.error])).toEqual([[true, null], [false, 'Error: thrown out of a timer'], [true, null]]);
    expect(emitted.map((row) => row.index)).toEqual([0, 1, 2]);
  });
});

describe('compile-time failures', () => {
  it('Given a syntax error on line 3, When the run is made, Then it is a compile error with that line and no rows', async () => {
    const { raw } = await judge(payload('var a = 1;\nvar b = 2;\nvar c = ;\n', [num(1)]));
    expect(raw).toMatchObject({ ok: false, error_kind: 'compile', error_at: { line: 3 }, results: [] });
    expect(raw.error).toMatch(/^CompileError: SyntaxError: /);
  });

  it('Given a throw at module level on line 2, When the run is made, Then it is a compile error naming the error and that line', async () => {
    const { raw } = await judge(payload('var a = 1;\nvar b = missing + a;\n', [num(1)]));
    expect(raw).toMatchObject({ ok: false, error_kind: 'compile', error: 'CompileError: ReferenceError: missing is not defined', error_at: { line: 2 }, results: [] });
  });

  it('Given a syntax error and an engine whose events carry no position, When the run is made, Then the compile error has no error_at', async () => {
    const { raw } = await judge(payload('var c = ;\n', [num(1)]), { lines: { event: false, stack: true } });
    expect(raw).toMatchObject({ ok: false, error_kind: 'compile' });
    expect(raw.error_at).toBeUndefined();
  });

  it('Given a throw inside a case and an engine whose stacks cannot be read, When the run is made, Then the row has the error and no error_at', async () => {
    const { raw } = await judge(payload('var twoSum = function() { throw new Error("x"); };', [num(1)]), { lines: { event: true, stack: false } });
    expect(raw.results?.[0]?.error).toBe('Error: x');
    expect(raw.results?.[0]?.error_at).toBeUndefined();
  });

  it('Given a throw inside a case, When the run is made, Then the row has error_at and nothing of the stack leaks into it', async () => {
    const { raw, emitted } = await judge(payload('var twoSum = function() {\n  throw new Error("x");\n};', [num(1)]));
    expect(raw.results?.[0]?.error_at).toMatchObject({ line: 2 });
    expect(Object.keys(raw.results?.[0] ?? {})).not.toContain('stack');
    expect(Object.keys(emitted[0]?.result ?? {})).not.toContain('stack');
  });

  it('Given neither a Solution class nor the function, When the run is made, Then it is a compile error naming what is missing', async () => {
    const { raw } = await judge(payload('var other = 1;', [num(1)]));
    expect(raw).toMatchObject({ ok: false, error_kind: 'compile', error: 'No Solution class or twoSum function defined', results: [] });
  });

  it('Given module-level output and two cases, When the run is made, Then the output is the first case’s stdout', async () => {
    const { raw } = await judge(payload('console.log("module level");\nvar twoSum = function(n) { console.log("case", n); return n; };', [num(1), num(2)]));
    expect(raw.results?.map((row) => row.stdout)).toEqual(['module level\ncase 1\n', 'case 2\n']);
  });
});
