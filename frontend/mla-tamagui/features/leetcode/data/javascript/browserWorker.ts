import type { WorkerSpawn } from './workerEngine';
import type { FromWorker, WorkerError } from './workerProtocol';

type ErrorLike = { message?: string; lineno?: number; colno?: number; preventDefault?(): void };
type BrowserWorker = {
  postMessage(message: unknown): void;
  terminate(): void;
  onmessage: ((event: { data: unknown }) => void) | null;
  onerror: ((event: ErrorLike) => void) | null;
};

/** The three browser globals a spawn needs, so a test can hand it fakes. */
export type WorkerEnvironment = {
  Blob: new (parts: string[], options: { type: string }) => unknown;
  URL: { createObjectURL(blob: unknown): string; revokeObjectURL(url: string): void };
  Worker: new (url: string) => BrowserWorker;
};

/** How the script reaches the worker: a blob url, or a data url for a WebView that refuses a worker from a blob. */
export type SpawnKind = 'blob' | 'data';

/** What an ErrorEvent says, without the engine's own prefix: Chromium writes `Uncaught ` before the message of an error no one caught. */
export function normalizeWorkerError(event: ErrorLike): WorkerError {
  const message = (event.message ?? '').replace(/^Uncaught\s+/, '');
  return { message, ...(event.lineno === undefined ? {} : { line: event.lineno }), ...(event.colno === undefined ? {} : { column: event.colno }) };
}

/**
 * Starts a worker on a script by a blob url or a data url. The worker's error is handed to the sink and cancelled, so it neither reaches
 * the page's own crash handler nor shows in the console as a failure of the app. The blob url is revoked when the worker is ended.
 */
export function createBrowserSpawn(env: WorkerEnvironment, kind: SpawnKind): WorkerSpawn {
  return (script, sink) => {
    const url = kind === 'blob' ? env.URL.createObjectURL(new env.Blob([script], { type: 'text/javascript' })) : `data:text/javascript;charset=utf-8,${encodeURIComponent(script)}`;
    const release = () => {
      if (kind === 'blob') env.URL.revokeObjectURL(url);
    };
    let worker: BrowserWorker;
    try {
      worker = new env.Worker(url);
    } catch (error) {
      release();
      throw error;
    }
    worker.onmessage = (event) => sink.message(event.data as FromWorker);
    worker.onerror = (event) => {
      event.preventDefault?.();
      sink.error(normalizeWorkerError(event));
    };
    return {
      scriptName: url,
      post: (message) => worker.postMessage(message),
      terminate() {
        worker.terminate();
        release();
      },
    };
  };
}
