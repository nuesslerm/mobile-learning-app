import { describe, expect, it } from 'vitest';

import { createBrowserSpawn, normalizeWorkerError, type WorkerEnvironment } from '../browserWorker';
import type { FromWorker, WorkerError } from '../workerProtocol';

type ErrorEventLike = { message?: string; lineno?: number; colno?: number; preventDefault?(): void };

/** The browser's three globals as fakes that remember what was asked of them. `refuse` is the address scheme the Worker constructor throws on. */
function fakeBrowser(refuse?: 'blob:' | 'data:') {
  const blobs: { parts: string[]; type: string }[] = [];
  const revoked: string[] = [];
  class FakeWorker {
    onmessage: ((event: { data: unknown }) => void) | null = null;
    onerror: ((event: ErrorEventLike) => void) | null = null;
    posted: unknown[] = [];
    terminated = false;
    constructor(readonly url: string) {
      if (refuse && url.startsWith(refuse)) throw new Error(`a worker from ${refuse} is refused`);
      workers.push(this);
    }
    postMessage(message: unknown) {
      this.posted.push(message);
    }
    terminate() {
      this.terminated = true;
    }
  }
  const workers: FakeWorker[] = [];
  const env: WorkerEnvironment = {
    Blob: class {
      constructor(parts: string[], options: { type: string }) {
        blobs.push({ parts, type: options.type });
      }
    } as unknown as WorkerEnvironment['Blob'],
    URL: {
      createObjectURL: () => `blob:fake/${blobs.length}`,
      revokeObjectURL: (url) => void revoked.push(url),
    },
    Worker: FakeWorker as unknown as WorkerEnvironment['Worker'],
  };
  return { env, workers, blobs, revoked };
}

const SINK = () => {
  const messages: FromWorker[] = [];
  const errors: WorkerError[] = [];
  return { messages, errors, sink: { message: (message: FromWorker) => void messages.push(message), error: (error: WorkerError) => void errors.push(error) } };
};

describe('starting a worker in the browser', () => {
  it('Given the blob kind, When a script is spawned, Then the worker starts on the blob url of a Blob that holds the script, and that url is its name', () => {
    const { env, workers, blobs } = fakeBrowser();
    const handle = createBrowserSpawn(env, 'blob')('var x = 1;', SINK().sink);
    expect(blobs).toEqual([{ parts: ['var x = 1;'], type: 'text/javascript' }]);
    expect(workers.map((worker) => worker.url)).toEqual(['blob:fake/1']);
    expect(handle.scriptName).toBe('blob:fake/1');
  });

  it('Given the data kind, When a script is spawned, Then the worker starts on a data url that holds the script and no Blob is made', () => {
    const { env, workers, blobs } = fakeBrowser();
    const script = "var s = 'é & #';\n";
    const handle = createBrowserSpawn(env, 'data')(script, SINK().sink);
    const url = workers[0]?.url ?? '';
    expect(blobs).toEqual([]);
    expect(url.startsWith('data:text/javascript;charset=utf-8,')).toBe(true);
    expect(decodeURIComponent(url.slice(url.indexOf(',') + 1))).toBe(script);
    expect(handle.scriptName).toBe(url);
  });

  it('Given a host that refuses a worker from a blob, When a script is spawned by blob, Then it throws, nothing is left running and the url is revoked', () => {
    const { env, workers, revoked } = fakeBrowser('blob:');
    expect(() => createBrowserSpawn(env, 'blob')('x', SINK().sink)).toThrow('a worker from blob: is refused');
    expect(workers).toEqual([]);
    expect(revoked).toEqual(['blob:fake/1']);
  });

  it('Given a host that refuses blobs, When the data kind spawns the same script, Then it starts', () => {
    const { env, workers } = fakeBrowser('blob:');
    createBrowserSpawn(env, 'data')('x', SINK().sink);
    expect(workers).toHaveLength(1);
  });

  it('Given a spawned worker, When it is ended, Then it is terminated and a blob url is revoked, and a data url has nothing to revoke', () => {
    const blob = fakeBrowser();
    createBrowserSpawn(blob.env, 'blob')('x', SINK().sink).terminate();
    expect([blob.workers[0]?.terminated, blob.revoked]).toEqual([true, ['blob:fake/1']]);

    const data = fakeBrowser();
    createBrowserSpawn(data.env, 'data')('x', SINK().sink).terminate();
    expect([data.workers[0]?.terminated, data.revoked]).toEqual([true, []]);
  });

  it('Given a spawned worker, When a message goes to it and it answers, Then the message reaches the worker and the answer reaches the sink', () => {
    const { env, workers } = fakeBrowser();
    const { sink, messages } = SINK();
    const handle = createBrowserSpawn(env, 'blob')('x', sink);
    handle.post({ type: 'case', index: 2, case: { inputs: {}, expected: 1 } });
    workers[0]?.onmessage?.({ data: { type: 'begin', index: 2 } });
    expect(workers[0]?.posted).toEqual([{ type: 'case', index: 2, case: { inputs: {}, expected: 1 } }]);
    expect(messages).toEqual([{ type: 'begin', index: 2 }]);
  });

  it('Given an error event with a position, When the worker raises it, Then the sink gets it normalized with the event’s own line and column, and the event is cancelled', () => {
    const { env, workers } = fakeBrowser();
    const { sink, errors } = SINK();
    createBrowserSpawn(env, 'blob')('x', sink);
    let cancelled = false;
    workers[0]?.onerror?.({ message: 'Uncaught SyntaxError: Unexpected token', lineno: 3, colno: 9, preventDefault: () => void (cancelled = true) });
    expect(errors).toEqual([{ message: 'SyntaxError: Unexpected token', line: 3, column: 9 }]);
    expect(cancelled).toBe(true);
  });
});

describe('the message of a worker’s error event', () => {
  it('Given Chromium’s Uncaught prefix, Then it is removed', () => {
    expect(normalizeWorkerError({ message: 'Uncaught ReferenceError: x is not defined', lineno: 2, colno: 1 })).toEqual({ message: 'ReferenceError: x is not defined', line: 2, column: 1 });
  });

  it('Given a thrown string and an engine with no prefix or position, Then the text stays and no position is made up', () => {
    expect(normalizeWorkerError({ message: 'just text' })).toEqual({ message: 'just text' });
  });

  it('Given an event with no message at all, Then the message is empty and the position is kept', () => {
    expect(normalizeWorkerError({ lineno: 4, colno: 2 })).toEqual({ message: '', line: 4, column: 2 });
  });
});
