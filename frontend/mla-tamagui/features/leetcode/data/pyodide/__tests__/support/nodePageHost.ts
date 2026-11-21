import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { createRpcServer } from '@lib/webview-rpc';
import { createLoopbackPair } from '@lib/webview-rpc/testing';
import { loadPyodide } from 'pyodide';

import { createPageHandlers } from '../../pageHandlers';
import type { PageHost } from '../../pageHost';

const harnessSource = readFileSync(join(dirname(new URL(import.meta.url).pathname), '../../harness/judge.py'), 'utf8');

/** A page host whose page is the real page handlers on a real Pyodide in Node: the runner speaks the real wire format to the real harness. */
export function createNodePageHost(): PageHost {
  const indexURL = dirname(createRequire(import.meta.url).resolve('pyodide/package.json'));
  const servers: { close(): void }[] = [];
  return {
    async prepare() {
      const pair = createLoopbackPair();
      const server = createRpcServer(
        pair.page,
        createPageHandlers({
          readFile: async () => new ArrayBuffer(0),
          boot: async () => ({ python: await loadPyodide({ indexURL, stdin: () => null, stdout: () => undefined, stderr: () => undefined, env: { PYTHONHASHSEED: '0' } }), importMs: 0, loadMs: 0 }),
          harnessSource,
          emit: (topic, data) => server.emit(topic, data),
          now: () => performance.now(),
          heapUsed: () => undefined,
        })
      );
      servers.push(server);
      return {
        transport: pair.host,
        files: { asm: 'node://asm', lock: 'node://lock', wasm: 'node://wasm', stdlib: 'node://stdlib' },
        show: () => queueMicrotask(() => server.emit('ready')),
        async *chunks() {
          yield { index: 0, total: 1, data: '' };
        },
        onLost: () => () => undefined,
      };
    },
    close() {
      for (const server of servers.splice(0)) server.close();
    },
  };
}
