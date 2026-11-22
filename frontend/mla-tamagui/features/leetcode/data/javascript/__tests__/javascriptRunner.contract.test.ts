import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it, vi } from 'vitest';

import { ProblemCatalog } from '../../../domain/problemCatalog';
import { runParamsFor } from '../../../domain/run/runParams';
import { createManualTimers } from '../../../testing/manualTimers';
import { createJavaScriptRunner } from '../javascriptRunner';
import { createNodePageHost } from './support/nodePageHost';
import { watched } from './support/judgeRun';
import { spawnNodeWorker } from './support/nodeWorkerSpawn';
import { signal } from './support/signal';

vi.setConfig({ testTimeout: 60_000 });

const named = JSON.parse(readFileSync(join(__dirname, 'fixtures/named.json'), 'utf8')) as Record<string, string>;
const source = (key: string): string => named[key] ?? '';
const twoSum = ProblemCatalog.getById('two-sum');
if (!twoSum) throw new Error('no two-sum');
const run = (code: string) => runParamsFor(twoSum, 'javascript', code);

/** The runner end to end: its queue and wire format, the real page handlers and the real harness on real worker threads. */
describe('the JavaScript runner on real workers', () => {
  const runner = createJavaScriptRunner({ host: createNodePageHost() });

  it('Given two-sum written as a class and as a var function, When each runs through the runner, Then both are Accepted over 12 cases', async () => {
    for (const key of ['TWO_SUM_CLASS', 'TWO_SUM_FUNCTION']) {
      expect(await runner.run(run(source(key))), key).toMatchObject({ source: 'offline', status_msg: 'Accepted', run_success: true, total_correct: 12, total_testcases: 12 });
    }
    expect(runner.state).toBe('ready');
  });

  it('Given a wrong answer, a raising solution and a syntax error, When they run on the same warm page, Then each verdict says what happened and the page stays loaded', async () => {
    const wrong = await runner.run(run('var twoSum = function() { return [0, 0]; };'));
    expect(wrong).toMatchObject({ status_msg: 'Wrong Answer' });
    const raising = await runner.run(run('var twoSum = function(nums) {\n  return nums.missing.length;\n};'));
    expect(raising).toMatchObject({ status_msg: 'Wrong Answer', runtime_error: expect.stringContaining('TypeError'), error_at: { line: 2 } });
    const broken = await runner.run(run('var twoSum = function(nums) {\n  return ;;\n  var = 1;\n};'));
    expect(broken).toMatchObject({ status_msg: 'Error', compile_error: expect.stringContaining('CompileError: SyntaxError') });
    expect(broken.error_at?.line).toBe(3);
  });

  it('Given a case that never ends, When its deadline fires after the worker began it, Then that row is a TimeoutError and the next run on the same page passes', async () => {
    const begins = signal<number>();
    const timers = createManualTimers();
    const quick = createJavaScriptRunner({
      host: createNodePageHost({ timers, spawn: watched(spawnNodeWorker, { message: (message) => message.type === 'begin' && begins.push(message.index) }) }),
    });
    const hung = quick.run(run(source('INFINITE_LOOP_ONE_CASE')));
    expect(await begins.next()).toBe(0);
    expect(await begins.next()).toBe(1);
    timers.fire(5000);
    expect(await hung).toMatchObject({ status_msg: 'Wrong Answer', runtime_error: 'TimeoutError: Code execution timed out (5s limit)' });
    expect(await quick.run(run(source('TWO_SUM_FUNCTION')))).toMatchObject({ status_msg: 'Accepted' });
    quick.dispose();
  });
});
