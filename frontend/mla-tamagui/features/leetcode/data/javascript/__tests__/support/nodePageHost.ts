import { createRpcServer, type Timers } from '@lib/webview-rpc';
import { createLoopbackPair } from '@lib/webview-rpc/testing';

import type { PageHost } from '../../../runner/pageHost';
import { createJsPageHandlers } from '../../pageHandlers';
import type { WorkerSpawn } from '../../workerEngine';
import { shippedPrelude } from './buildPrelude';
import { spawnNodeWorker } from './nodeWorkerSpawn';

const realTimers: Timers = { set: (fn, ms) => setTimeout(fn, ms), clear: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>) };

/**
 * A page host whose page is the real page handlers over real worker threads: the runner speaks the real wire format to the real harness.
 * `timers` are the page's own (the case deadlines), so a test can hold them; `spawn` lets it watch the workers.
 */
export function createNodePageHost(options: { timers?: Timers; spawn?: WorkerSpawn } = {}): PageHost {
  const servers: { close(): void }[] = [];
  const spawn = options.spawn ?? spawnNodeWorker;
  return {
    async prepare() {
      const pair = createLoopbackPair();
      const server = createRpcServer(
        pair.page,
        createJsPageHandlers({
          prelude: await shippedPrelude(),
          spawns: { blob: spawn, data: spawn },
          emit: (topic, data) => server.emit(topic, data),
          timers: options.timers ?? realTimers,
        })
      );
      servers.push(server);
      return {
        transport: pair.host,
        files: {},
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
