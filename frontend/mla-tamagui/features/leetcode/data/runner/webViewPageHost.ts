import { createWebViewHostTransport, type WebViewHost } from '@lib/webview-rpc';

import type { PageHost, PageSession, PageSource } from './pageHost';

const NO_CHUNKS: AsyncIterable<never> = { [Symbol.asyncIterator]: async function* () {} };

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
  /** what the page is loaded from and the urls of its runtime files, resolved once per host */
  resolve: () => Promise<{ source: PageSource; files: Record<string, string> }>;
  /** a runtime file in base64 pieces, for a page that cannot read it itself */
  chunks?: (uri: string) => AsyncIterable<{ index: number; total: number; data: string }>;
}): { host: PageHost; view: PageView } {
  const listeners = new Set<() => void>();
  const lostListeners = new Set<(reason: string) => void>();
  let resolved: ReturnType<typeof config.resolve> | null = null;
  let generation = 0;
  let current: ReturnType<typeof createWebViewHostTransport> | null = null;
  let handle: WebViewHost | null = null;
  let shown: PageSnapshot | null = null;
  const notify = () => listeners.forEach((listener) => listener());

  return {
    host: {
      async prepare(): Promise<PageSession> {
        // the assets are resolved once per provider; a failure is not remembered, so the next run can try again
        resolved ??= config.resolve();
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
          chunks: (name) => config.chunks?.(files[name] ?? '') ?? NO_CHUNKS,
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
