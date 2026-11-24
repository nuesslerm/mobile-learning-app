import { describe, expect, it } from 'vitest';

import type { TestCase } from '../../../domain/run/execution';
import { createManualTimers } from '../../../testing/manualTimers';
import { createWorkerEngine, type WorkerSink, type WorkerSpawn } from '../workerEngine';
import type { CaseRow, ToWorker } from '../workerProtocol';
import { signal } from './support/signal';

/** A worker the test plays: it says what the test tells it to, when the test tells it to, and records what the engine posted and whether it was ended. */
type Fake = { name: string; script: string; sink: WorkerSink; posted: ReturnType<typeof signal<ToWorker>>; terminated: boolean };

function theater(options: { failFrom?: number; failPost?: boolean } = {}) {
  const timers = createManualTimers();
  const workers: Fake[] = [];
  const spawned = signal<Fake>();
  const spawn: WorkerSpawn = (script, sink) => {
    if (options.failFrom !== undefined && workers.length >= options.failFrom) throw new Error('Worker is not defined');
    const worker: Fake = { name: `fake:${workers.length + 1}`, script, sink, posted: signal<ToWorker>(), terminated: false };
    workers.push(worker);
    spawned.push(worker);
    return {
      scriptName: worker.name,
      post: (message) => {
        if (options.failPost) throw new Error('the worker port is closed');
        worker.posted.push(message);
      },
      terminate: () => {
        worker.terminated = true;
      },
    };
  };
  return { timers, workers, spawned, engine: createWorkerEngine({ spawn, timers }) };
}

const ROW: CaseRow = { pass: true, input: {}, actual: 1, expected: 1, stdout: '', stderr: '', error: null, elapsed_ms: 1 };
const CASE: TestCase = { inputs: {}, expected: 1 };

async function loaded() {
  const play = theater();
  const loading = play.engine.load('the script', 5000);
  const worker = await play.spawned.next();
  worker.sink.message({ type: 'loaded', stdout: '' });
  await loading;
  return { ...play, worker };
}

describe('loading the learner’s code', () => {
  it('Given a worker that says it has loaded, When load runs, Then the outcome carries its module-level output, the script was the worker’s and no timer is left', async () => {
    const { engine, spawned, timers } = theater();
    const loading = engine.load('script one', 5000);
    const worker = await spawned.next();
    expect(timers.pending).toEqual([5000]);
    worker.sink.message({ type: 'loaded', stdout: 'boot\n' });
    expect(await loading).toEqual({ kind: 'loaded', stdout: 'boot\n' });
    expect(worker.script).toBe('script one');
    expect(timers.pending).toEqual([]);
  });

  it('Given a worker that loaded with nothing to call, When load runs, Then the outcome is missing with the message and the worker is ended', async () => {
    const { engine, spawned } = theater();
    const loading = engine.load('s', 5000);
    const worker = await spawned.next();
    worker.sink.message({ type: 'loaded', stdout: '', error: 'No Solution class or twoSum function defined' });
    expect(await loading).toEqual({ kind: 'missing', message: 'No Solution class or twoSum function defined' });
    expect(worker.terminated).toBe(true);
  });

  it('Given a worker whose script raised an error event, When load runs, Then the outcome is failed with that error and the worker is ended', async () => {
    const { engine, spawned } = theater();
    const loading = engine.load('s', 5000);
    const worker = await spawned.next();
    worker.sink.error({ message: 'SyntaxError: Unexpected token', line: 3, column: 9 });
    expect(await loading).toEqual({ kind: 'failed', error: { message: 'SyntaxError: Unexpected token', line: 3, column: 9 } });
    expect(worker.terminated).toBe(true);
  });

  it('Given a worker that never loads, When the module-level deadline fires, Then the outcome is timeout and the worker is ended', async () => {
    const { engine, spawned, timers } = theater();
    const loading = engine.load('s', 5000);
    const worker = await spawned.next();
    expect(timers.fire(5000)).toBe(1);
    expect(await loading).toEqual({ kind: 'timeout' });
    expect(worker.terminated).toBe(true);
  });

  it('Given a host that cannot start a worker, When load runs, Then the outcome is died and not a hang', async () => {
    const { engine } = theater({ failFrom: 0 });
    expect(await engine.load('s', 5000)).toEqual({ kind: 'died', reason: 'cannot start a worker: Worker is not defined' });
  });

  it('Given a second load, When it starts, Then the first worker is ended', async () => {
    const { engine, workers, spawned } = theater();
    void engine.load('one', 5000);
    await spawned.next();
    void engine.load('two', 5000);
    await spawned.next();
    expect(workers.map((worker) => worker.terminated)).toEqual([true, false]);
  });
});

describe('running a case', () => {
  it('Given a loaded worker, When a case runs, Then the case is posted and its row is the outcome, with the name of the worker that ran it', async () => {
    const { engine, worker, timers } = await loaded();
    const running = engine.runCase(0, CASE, 5000);
    expect(await worker.posted.next()).toEqual({ type: 'case', index: 0, case: CASE });
    worker.sink.message({ type: 'begin', index: 0 });
    worker.sink.message({ type: 'row', index: 0, row: ROW });
    expect(await running).toEqual({ kind: 'row', row: ROW, scriptName: 'fake:1' });
    expect(timers.pending).toEqual([]);
  });

  it('Given a case that is posted, When the worker says it has begun, Then the deadline is armed again from that moment and no timer outlives the case', async () => {
    const { engine, worker, timers } = await loaded();
    const before = timers.armed.length;
    const running = engine.runCase(0, CASE, 4000);
    await worker.posted.next();
    expect(timers.pending).toEqual([4000]);
    worker.sink.message({ type: 'begin', index: 0 });
    worker.sink.message({ type: 'row', index: 0, row: ROW });
    await running;
    expect(timers.armed.slice(before)).toEqual([4000, 4000]);
    expect(timers.pending).toEqual([]);
  });

  it('Given a case that never ends, When its deadline fires after the worker began it, Then the outcome is timeout and the worker is ended', async () => {
    const { engine, worker, timers } = await loaded();
    const running = engine.runCase(0, CASE, 5000);
    await worker.posted.next();
    worker.sink.message({ type: 'begin', index: 0 });
    timers.fire(5000);
    expect(await running).toEqual({ kind: 'timeout' });
    expect(worker.terminated).toBe(true);
  });

  it('Given a worker too busy to begin the posted case, When the deadline fires, Then it is ended all the same', async () => {
    const { engine, worker, timers } = await loaded();
    const running = engine.runCase(0, CASE, 5000);
    await worker.posted.next();
    expect(timers.fire(5000)).toBe(1);
    expect(await running).toEqual({ kind: 'timeout' });
    expect(worker.terminated).toBe(true);
  });

  it('Given a timeout, When the next case runs, Then a new worker loads the same script first and the case runs there', async () => {
    const { engine, worker, workers, timers, spawned } = await loaded();
    const first = engine.runCase(0, CASE, 5000);
    await worker.posted.next();
    timers.fire(5000);
    await first;

    const second = engine.runCase(1, CASE, 5000);
    const next = await spawned.next();
    expect(next.script).toBe('the script');
    expect(workers).toHaveLength(2);
    next.sink.message({ type: 'loaded', stdout: 'not shown again\n' });
    expect(await next.posted.next()).toEqual({ type: 'case', index: 1, case: CASE });
    next.sink.message({ type: 'begin', index: 1 });
    next.sink.message({ type: 'row', index: 1, row: ROW });
    expect(await second).toEqual({ kind: 'row', row: ROW, scriptName: 'fake:2' });
  });

  it('Given a worker that was ended, When it sends a late row, Then it cannot answer a case of the next worker', async () => {
    const { engine, worker, timers, spawned } = await loaded();
    const first = engine.runCase(0, CASE, 5000);
    await worker.posted.next();
    timers.fire(5000);
    await first;

    const second = engine.runCase(0, CASE, 5000);
    const next = await spawned.next();
    next.sink.message({ type: 'loaded', stdout: '' });
    await next.posted.next();
    worker.sink.message({ type: 'row', index: 0, row: { ...ROW, actual: 'late' } });
    next.sink.message({ type: 'begin', index: 0 });
    next.sink.message({ type: 'row', index: 0, row: ROW });
    expect(await second).toEqual({ kind: 'row', row: ROW, scriptName: 'fake:2' });
  });

  it('Given a begin or a row for another case, When it arrives, Then it does not answer this one', async () => {
    const { engine, worker } = await loaded();
    const running = engine.runCase(1, CASE, 5000);
    await worker.posted.next();
    worker.sink.message({ type: 'row', index: 0, row: { ...ROW, actual: 'other' } });
    worker.sink.message({ type: 'begin', index: 1 });
    worker.sink.message({ type: 'row', index: 1, row: ROW });
    expect(await running).toEqual({ kind: 'row', row: ROW, scriptName: 'fake:1' });
  });

  it('Given an uncaught error of the worker while a case is in flight, When the error event arrives, Then the outcome is raised with it and the worker is ended', async () => {
    const { engine, worker } = await loaded();
    const running = engine.runCase(0, CASE, 5000);
    await worker.posted.next();
    worker.sink.error({ message: 'Error: thrown out of a timer', line: 4, column: 3 });
    expect(await running).toEqual({ kind: 'raised', error: { message: 'Error: thrown out of a timer', line: 4, column: 3 } });
    expect(worker.terminated).toBe(true);
  });

  it('Given a worker whose port refuses the case, When the case runs, Then the outcome is died and the worker is ended', async () => {
    const play = theater({ failPost: true });
    const loading = play.engine.load('s', 5000);
    const worker = await play.spawned.next();
    worker.sink.message({ type: 'loaded', stdout: '' });
    await loading;
    expect(await play.engine.runCase(0, CASE, 5000)).toEqual({ kind: 'died', reason: 'the worker port is closed' });
    expect(worker.terminated).toBe(true);
    expect(play.timers.pending).toEqual([]);
  });
});

describe('a worker that has to be started again', () => {
  it('Given a timeout and a host that can start no more workers, When the next case runs, Then the outcome is died and not a hang', async () => {
    const play = theater({ failFrom: 1 });
    const loading = play.engine.load('s', 5000);
    const worker = await play.spawned.next();
    worker.sink.message({ type: 'loaded', stdout: '' });
    await loading;
    const first = play.engine.runCase(0, CASE, 5000);
    await worker.posted.next();
    play.timers.fire(5000);
    await first;
    expect(await play.engine.runCase(1, CASE, 5000)).toEqual({ kind: 'died', reason: 'cannot start a worker: Worker is not defined' });
  });

  it('Given a timeout and a new worker whose module-level code never ends, When the next case runs, Then that case is a timeout', async () => {
    const { engine, worker, timers, spawned } = await loaded();
    const first = engine.runCase(0, CASE, 5000);
    await worker.posted.next();
    timers.fire(5000);
    await first;
    const second = engine.runCase(1, CASE, 5000);
    await spawned.next();
    timers.fire(5000);
    expect(await second).toEqual({ kind: 'timeout' });
  });

  it('Given a timeout and a new worker that fails to load, When the next case runs, Then the outcome is died with the worker’s error', async () => {
    const { engine, worker, timers, spawned } = await loaded();
    const first = engine.runCase(0, CASE, 5000);
    await worker.posted.next();
    timers.fire(5000);
    await first;
    const second = engine.runCase(1, CASE, 5000);
    const next = await spawned.next();
    next.sink.error({ message: 'ReferenceError: later is not defined' });
    expect(await second).toEqual({ kind: 'died', reason: 'ReferenceError: later is not defined' });
  });
});

describe('ending the engine', () => {
  it('Given a live worker, When dispose is called, Then it is ended and the engine can load again', async () => {
    const { engine, worker, spawned } = await loaded();
    engine.dispose();
    expect(worker.terminated).toBe(true);
    const again = engine.load('t', 5000);
    const next = await spawned.next();
    next.sink.message({ type: 'loaded', stdout: '' });
    expect(await again).toEqual({ kind: 'loaded', stdout: '' });
  });
});
