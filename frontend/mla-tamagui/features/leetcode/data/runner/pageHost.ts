import type { RpcTransport } from '@lib/webview-rpc';

/** Where the hidden WebView loads the page from: a file beside read access to the assets on a phone, html on the web (a base url gives that html an origin). */
export type PageSource = { uri: string; directory: string } | { html: string; baseUrl?: string };

/** One load of the hidden page: its own transport, so a late message of a discarded page cannot settle a call of the next one. */
export type PageSession = {
  readonly transport: RpcTransport;
  /** where the page can read each runtime file from; none for a page that needs no files */
  readonly files: Record<string, string>;
  /** mounts the page; called after the engine subscribed to its `ready` event, so the one `ready` cannot be missed */
  show(): void;
  /** the files in base64 pieces: the fallback when the page cannot read them itself */
  chunks(name: string): AsyncIterable<{ index: number; total: number; data: string }>;
  /** the page's process died or its load failed; returns what stops listening */
  onLost(listener: (reason: string) => void): () => void;
};

/** A hidden page the runner can load, talk to over rpc, and throw away. */
export interface PageHost {
  /** resolves the assets once and makes a fresh session; rejects with a CodeRunnerError of kind unavailable, stage assets or page */
  prepare(): Promise<PageSession>;
  /** removes the page; safe when there is none */
  close(): void;
}
