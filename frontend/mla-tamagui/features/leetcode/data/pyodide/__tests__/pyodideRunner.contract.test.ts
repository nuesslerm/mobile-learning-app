import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it, vi } from 'vitest';

import { ProblemCatalog } from '../../../domain/problemCatalog';
import { runParamsFor } from '../../../domain/runParams';
import { createPyodideRunner } from '../pyodideRunner';
import { createNodePageHost } from './support/nodePageHost';

vi.setConfig({ testTimeout: 120_000, hookTimeout: 120_000 });

const goldens = JSON.parse(readFileSync(join(__dirname, 'fixtures/referenceSolutions.json'), 'utf8')) as { problem: string; code: string }[];
const named = JSON.parse(readFileSync(join(__dirname, 'fixtures/named.json'), 'utf8')) as Record<string, string>;
const twoSum = ProblemCatalog.getById('two-sum');
if (!twoSum) throw new Error('no two-sum');

/** The runner end to end: its queue and wire format, the real page handlers and the real harness on a real interpreter. */
describe('the Pyodide runner on a real interpreter', () => {
  const runner = createPyodideRunner({ host: createNodePageHost() });

  it('Given reference solutions across linked lists, trees, in-place matrices and plain arrays, When each runs through the runner, Then every one is Accepted over its bundled cases', async () => {
    for (const id of ['reverse-linked-list', 'invert-binary-tree', 'rotate-image', 'set-matrix-zeroes', 'valid-parentheses', 'merge-intervals', 'course-schedule']) {
      const problem = ProblemCatalog.getById(id);
      const code = goldens.find((g) => g.problem === id)?.code ?? '';
      const response = await runner.run(runParamsFor(problem ?? twoSum, 'python3', code));
      expect(response, id).toMatchObject({ source: 'offline', status_msg: 'Accepted', run_success: true });
      expect(response.total_testcases, id).toBeGreaterThan(0);
    }
    expect(runner.state).toBe('ready');
  });

  it('Given a wrong answer, a raising solution and a syntax error, When they run on the same warm page, Then each verdict says what happened and the page stays loaded', async () => {
    const wrong = await runner.run(runParamsFor(twoSum, 'python3', named.TWO_SUM_WRONG ?? ''));
    expect(wrong).toMatchObject({ status_msg: 'Wrong Answer' });
    const raising = await runner.run(runParamsFor(twoSum, 'python3', named.EXCEPTION_AT_COLUMN ?? ''));
    expect(raising).toMatchObject({ status_msg: 'Wrong Answer', runtime_error: "NameError: name 'undefined_name' is not defined", error_at: { line: 4 } });
    const broken = await runner.run(runParamsFor(twoSum, 'python3', named.SYNTAX_ERROR ?? ''));
    expect(broken).toMatchObject({ status_msg: 'Error', compile_error: expect.stringContaining('CompileError') });
    expect(broken.error_at?.line).toBeGreaterThan(0);
  });

  it('Given a case that never ends, When the run is made with a short limit, Then the soft deadline reports it for that case and the next run on the same page passes', async () => {
    const quick = createPyodideRunner({ host: createNodePageHost(), limits: { caseMs: 1000 } });
    const hung = await quick.run(runParamsFor(twoSum, 'python3', named.INFINITE_LOOP_ONE_CASE ?? ''));
    expect(hung).toMatchObject({ status_msg: 'Wrong Answer', runtime_error: 'TimeoutError: Code execution timed out (1s limit)' });
    expect(await quick.run(runParamsFor(twoSum, 'python3', named.TWO_SUM ?? ''))).toMatchObject({ status_msg: 'Accepted' });
    quick.dispose();
  });
});
