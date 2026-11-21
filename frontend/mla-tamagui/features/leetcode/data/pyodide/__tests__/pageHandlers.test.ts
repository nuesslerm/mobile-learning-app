import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { createRpcClient, createRpcServer } from '@lib/webview-rpc';
import { createLoopbackPair } from '@lib/webview-rpc/testing';
import { loadPyodide, type PyodideInterface } from 'pyodide';
import { describe, expect, it, vi } from 'vitest';

import { ProblemCatalog } from '../../../domain/problemCatalog';
import { rawRunSchema } from '../../../domain/run/verdict';
import type { JudgeParams } from '../judgeProtocol';
import { createPageHandlers, type PageDeps } from '../pageHandlers';

vi.setConfig({ testTimeout: 60_000, hookTimeout: 120_000 });

const harnessSource = readFileSync(join(__dirname, '../harness/judge.py'), 'utf8');
const base64Of = (bytes: Uint8Array) => Buffer.from(bytes).toString('base64');

function mount(deps: Partial<PageDeps> & Pick<PageDeps, 'boot'>) {
  const events: { topic: string; data: unknown }[] = [];
  const pair = createLoopbackPair();
  const handlers = createPageHandlers({ readFile: async () => new ArrayBuffer(0), harnessSource, emit: (topic, data) => events.push({ topic, data }), now: () => 0, heapUsed: () => undefined, ...deps });
  createRpcServer(pair.page, handlers);
  const client = createRpcClient(pair.host, { timeoutMs: 60_000 });
  return { client, events };
}

describe('the page handlers', () => {
  it('Given no boot yet, When the judge is called, Then it fails saying the interpreter has not booted', async () => {
    const { client } = mount({ boot: async () => { throw new Error('not reached'); } });
    await expect(client.call('judge', { runId: 1, payload: { code: '', methodName: 'm', testCases: [] } })).rejects.toMatchObject({ code: 'remote', message: 'the interpreter has not booted' });
  });

  it('Given a file pulled by url and one pushed in chunks, When the interpreter boots, Then boot receives both whole', async () => {
    // Given the page holds one file it pulled and one it was sent in three pieces
    let seen: Record<string, number> = {};
    const fakePython = { version: 'fake', globals: { get: () => () => ({ get: () => () => '{}' }) }, runPython: () => undefined } as unknown as PyodideInterface;
    const { client } = mount({
      readFile: async () => new Uint8Array([1, 2, 3, 4]).buffer,
      boot: async (files) => {
        seen = Object.fromEntries(Object.entries(files).map(([name, bytes]) => [name, bytes?.length ?? 0]));
        return { python: fakePython, importMs: 1, loadMs: 2 };
      },
    });
    await client.call('pull', { asm: 'file:///a', lock: 'file:///l', wasm: 'file:///w', stdlib: 'file:///s' });
    const whole = new Uint8Array(10).map((_, i) => i);
    for (const [index, part] of [whole.slice(0, 4), whole.slice(4, 8), whole.slice(8)].entries()) await client.call('chunk', { name: 'asm', index, total: 3, data: base64Of(part) });

    // When it boots
    const reply = await client.call<{ importMs: number; loadMs: number; version: string }>('boot');

    // Then the chunked file is one 10-byte file again, and the pulled ones are whole
    expect(seen).toEqual({ asm: 10, lock: 4, wasm: 4, stdlib: 4 });
    expect(reply).toEqual({ importMs: 1, loadMs: 2, version: 'fake' });
  });

  it('Given a booted real interpreter, When the judge runs a solution over the bridge, Then the answer is the harness report and each case was emitted as it finished', async () => {
    // Given the real handlers with a Node interpreter
    const indexURL = dirname(createRequire(import.meta.url).resolve('pyodide/package.json'));
    const { client, events } = mount({ boot: async () => ({ python: await loadPyodide({ indexURL, stdin: () => null, stdout: () => undefined, stderr: () => undefined }), importMs: 0, loadMs: 0 }) });
    const { version } = await client.call<{ version: string }>('boot');
    const cases = (ProblemCatalog.getById('two-sum')?.testCases ?? []).slice(0, 3);
    const code = 'class Solution:\n    def twoSum(self, nums, target):\n        seen = {}\n        for i, n in enumerate(nums):\n            if target - n in seen:\n                return [seen[target - n], i]\n            seen[n] = i\n';
    const params: JudgeParams = { runId: 7, payload: { code, methodName: 'twoSum', testCases: cases } };

    // When the judge runs
    const answer = rawRunSchema.parse(JSON.parse(await client.call<string>('judge', params)));

    // Then it is the harness's report, and the cases were emitted in order, tagged with the run
    expect(version).toBe('314.0.8');
    expect(answer.ok).toBe(true);
    expect(answer.results).toHaveLength(3);
    const emitted = events.filter((e) => e.topic === 'case').map((e) => e.data as { runId: number; row: string });
    expect(emitted.map((e) => [e.runId, (JSON.parse(e.row) as { index: number }).index])).toEqual([[7, 0], [7, 1], [7, 2]]);
  });
});
