import { createRpcClient, createRpcServer } from '@lib/webview-rpc';
import { createLoopbackPair } from '@lib/webview-rpc/testing';
import { describe, expect, it, vi } from 'vitest';

import { ProblemCatalog } from '../../../domain/problemCatalog';
import { rawRunSchema } from '../../../domain/run/verdict';
import { createManualTimers } from '../../../testing/manualTimers';
import type { JudgeParams } from '../../runner/judgeProtocol';
import { createJsPageHandlers, type JsPageDeps } from '../pageHandlers';
import type { WorkerSpawn } from '../workerEngine';
import { shippedPrelude } from './support/buildPrelude';
import { spawnNodeWorker } from './support/nodeWorkerSpawn';
import { signal } from './support/signal';

vi.setConfig({ testTimeout: 60_000 });

async function mount(overrides: Partial<JsPageDeps> = {}) {
  const events: { topic: string; data: unknown }[] = [];
  const timers = createManualTimers();
  const pair = createLoopbackPair();
  const handlers = createJsPageHandlers({
    prelude: await shippedPrelude(),
    spawns: { blob: spawnNodeWorker, data: spawnNodeWorker },
    emit: (topic, data) => events.push({ topic, data }),
    timers,
    ...overrides,
  });
  createRpcServer(pair.page, handlers);
  return { client: createRpcClient(pair.host, { timeoutMs: 60_000 }), events, timers };
}

const refusing: WorkerSpawn = () => {
  throw new Error('Worker is not defined');
};
const TWO_SUM = 'var twoSum = function(nums, target) {\n  const seen = new Map();\n  for (let i = 0; i < nums.length; i += 1) {\n    if (seen.has(target - nums[i])) return [seen.get(target - nums[i]), i];\n    seen.set(nums[i], i);\n  }\n};\n';

describe('the page handlers: ping and boot', () => {
  it('Given the page, When it is pinged, Then it answers with a number', async () => {
    const { client } = await mount();
    expect(typeof (await client.call<number>('ping'))).toBe('number');
  });

  it('Given a host whose workers start and report positions, When the page boots, Then it answers the blob way with both line mechanisms on, and no timer is left', async () => {
    const { client, timers } = await mount();
    expect(await client.call('boot')).toEqual({ spawn: 'blob', eventLines: true, stackLines: true });
    expect(timers.pending).toEqual([]);
  });

  it('Given a host that refuses a worker from a blob, When the page boots, Then it answers the data way', async () => {
    const { client } = await mount({ spawns: { blob: refusing, data: spawnNodeWorker } });
    expect(await client.call('boot')).toMatchObject({ spawn: 'data' });
  });

  it('Given a host that starts no worker either way, When the page boots, Then it fails saying it cannot start a worker, and names both ways', async () => {
    const { client } = await mount({ spawns: { blob: refusing, data: refusing } });
    await expect(client.call('boot')).rejects.toMatchObject({
      code: 'remote',
      message: 'cannot start a worker (blob: Worker is not defined; data: Worker is not defined)',
    });
  });

  it('Given workers that never answer, When the probe’s deadline passes for each way, Then the page fails saying it cannot start a worker', async () => {
    const spawned = signal<string>();
    const silent: WorkerSpawn = (_script, _sink) => {
      spawned.push('started');
      return { scriptName: 'silent', post: () => undefined, terminate: () => undefined };
    };
    const { client, timers } = await mount({ spawns: { blob: silent, data: silent } });
    const booting = client.call('boot');
    const settled = booting.then(
      () => undefined,
      (error: unknown) => error
    );
    await spawned.next();
    expect(timers.fire(10_000)).toBe(1);
    await spawned.next();
    expect(timers.fire(10_000)).toBe(1);
    expect(await settled).toMatchObject({ code: 'remote', message: 'cannot start a worker (blob: it did not answer; data: it did not answer)' });
  });

  it('Given an engine whose error events carry no position, When the page boots, Then it says so and the stack mechanism is still on', async () => {
    const withoutPositions: WorkerSpawn = (script, sink) => spawnNodeWorker(script, { message: sink.message, error: (error) => sink.error({ message: error.message }) });
    const { client } = await mount({ spawns: { blob: withoutPositions, data: withoutPositions } });
    expect(await client.call('boot')).toEqual({ spawn: 'blob', eventLines: false, stackLines: true });
  });

  it('Given an engine whose stacks do not name the script, When the page boots, Then it says so and the event mechanism is still on', async () => {
    const withoutFrames: WorkerSpawn = (script, sink) =>
      spawnNodeWorker(script, {
        message: (message) => sink.message(typeof message === 'string' ? ('Error: probe\n    at <anonymous>' as never) : message),
        error: sink.error,
      });
    const { client } = await mount({ spawns: { blob: withoutFrames, data: withoutFrames } });
    expect(await client.call('boot')).toEqual({ spawn: 'blob', eventLines: true, stackLines: false });
  });
});

describe('the page handlers: a judge call', () => {
  it('Given no boot yet, When the judge is called, Then it fails saying the page has not booted', async () => {
    const { client } = await mount();
    await expect(client.call('judge', { runId: 1, payload: { code: '', methodName: 'm', testCases: [] } })).rejects.toMatchObject({ code: 'remote', message: 'the page has not booted' });
  });

  it('Given a booted page, When the judge runs two-sum over the bridge, Then the answer parses as a raw run and each case was emitted as it finished, in order, tagged with the run', async () => {
    const { client, events } = await mount();
    await client.call('boot');
    const cases = (ProblemCatalog.getById('two-sum')?.testCases ?? []).slice(0, 3);
    const params: JudgeParams = { runId: 7, payload: { code: TWO_SUM, methodName: 'twoSum', testCases: cases } };

    const raw = rawRunSchema.parse(JSON.parse(await client.call<string>('judge', params)));

    expect(raw.ok).toBe(true);
    expect(raw.results?.map((row) => row.pass)).toEqual([true, true, true]);
    const cased = events.filter((event) => event.topic === 'case').map((event) => event.data as { runId: number; row: string });
    expect(cased.map((event) => event.runId)).toEqual([7, 7, 7]);
    expect(cased.map((event) => (JSON.parse(event.row) as { index: number }).index)).toEqual([0, 1, 2]);
    expect(cased.map((event) => (JSON.parse(event.row) as { result: unknown }).result)).toEqual(raw.results);
  });

  it('Given a booted page, When the judge runs code with a syntax error, Then the answer is a compile error with its line', async () => {
    const { client } = await mount();
    await client.call('boot');
    const params: JudgeParams = { runId: 1, payload: { code: 'var a = 1;\nvar b = ;\n', methodName: 'twoSum', testCases: [{ inputs: {}, expected: 1 }] } };
    const raw = rawRunSchema.parse(JSON.parse(await client.call<string>('judge', params)));
    expect(raw).toMatchObject({ ok: false, error_kind: 'compile', error_at: { line: 2 } });
  });

  it('Given a booted page and a run, When the next run is made on the same page, Then it answers too: a run leaves nothing behind', async () => {
    const { client } = await mount();
    await client.call('boot');
    const payload = { code: 'var twoSum = function() { return 1; };', methodName: 'twoSum', testCases: [{ inputs: {}, expected: 1 }] };
    for (const runId of [1, 2]) {
      const raw = rawRunSchema.parse(JSON.parse(await client.call<string>('judge', { runId, payload })));
      expect(raw.results?.[0]?.pass).toBe(true);
    }
  });
});
