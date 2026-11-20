import { createWebViewHostTransport, type WebViewHost } from '@lib/webview-rpc';

import type { PyodideFileName } from './pageHandlers';
import type { PageHost, PageSession } from './pageHost';
import { resolvePage, type AssetIo, type PageSource } from './pyodideAssets';

export type PageSnapshot = { generation: number; source: PageSource };

/** What a view needs to show the hidden page: when to mount it, the handle to drive it, and where its messages and its death go. */
export type PageView = {
  subscribe(listener: () => void): () => void;
  /** null while no page is shown; the same object until the page changes, as a store snapshot must be */
  snapshot(): PageSnapshot | null;
  bind(handle: WebViewHost | null): void;
  /** a message of the page of `generation`; one of an older page is dropped */
  receive(generation: number, raw: string): void;
  lost(generation: number, reason: string): void;
};

/** A PageHost over a view store (no React): each prepare is a new generation with its own transport, so a late message of a discarded page cannot reach the next. */
export function createWebViewPageHost(config: {
  modules: () => Promise<Record<PyodideFileName, number>>;
  pageHtml: string;
  io: AssetIo;
}): { host: PageHost; view: PageView } {
  const listeners = new Set<() => void>();
  const lostListeners = new Set<(reason: string) => void>();
  let resolved: ReturnType<typeof resolvePage> | null = null;
  let generation = 0;
  let current: ReturnType<typeof createWebViewHostTransport> | null = null;
  let handle: WebViewHost | null = null;
  let shown: PageSnapshot | null = null;
  const notify = () => listeners.forEach((listener) => listener());

  return {
    host: {
      async prepare(): Promise<PageSession> {
        // the assets are resolved once per provider; a failure is not remembered, so the next run can try again
        resolved ??= config
          .modules()
          .then((modules) => resolvePage(modules, config.pageHtml, config.io));
        const { source, files } = await resolved.catch((error: unknown) => {
          resolved = null;
          throw error;
        });
        generation += 1;
        const mine = generation;
        const transport = createWebViewHostTransport(() =>
          generation === mine ? handle : null
        );
        current = transport;
        lostListeners.clear();
        return {
          transport,
          files,
          show() {
            shown = { generation: mine, source };
            notify();
          },
          chunks: (name) => config.io.chunks(files[name]),
          onLost(listener) {
            lostListeners.add(listener);
            return () => void lostListeners.delete(listener);
          },
        };
      },
      close() {
        generation += 1;
        current = null;
        shown = null;
        notify();
      },
    },
    view: {
      subscribe(listener) {
        listeners.add(listener);
        return () => void listeners.delete(listener);
      },
      snapshot: () => shown,
      bind(next) {
        handle = next;
      },
      receive(fromGeneration, raw) {
        if (fromGeneration === generation) current?.receive(raw);
      },
      lost(fromGeneration, reason) {
        if (fromGeneration === generation)
          for (const listener of [...lostListeners]) listener(reason);
      },
    },
  };
}
