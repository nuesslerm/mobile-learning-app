// The page the hidden WebView runs for JavaScript: it answers the app's calls over @lib/webview-rpc and starts the workers that run the
// learner's code (handlers in pageHandlers.ts). Compiled to jsPage.ts by scripts/js-runner-bundle.mjs.
import { createPageTransport, createRpcServer, type PageWindow, type Timers } from '@lib/webview-rpc';

import { createBrowserSpawn, type WorkerEnvironment } from './browserWorker';
import { createJsPageHandlers } from './pageHandlers';

const timers: Timers = { set: (fn, ms) => setTimeout(fn, ms), clear: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>) };
// read from the global object, so a WebView with no Worker fails when a worker is asked for and the page still says it is ready
const environment = globalThis as unknown as WorkerEnvironment;

const server = createRpcServer(
  createPageTransport(window as unknown as PageWindow),
  createJsPageHandlers({
    prelude: __JS_RUNNER_PRELUDE__,
    spawns: { blob: createBrowserSpawn(environment, 'blob'), data: createBrowserSpawn(environment, 'data') },
    emit: (topic, data) => server.emit(topic, data),
    timers,
  })
);
server.emit('ready');
