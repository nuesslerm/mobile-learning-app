import { RpcError } from '../core/protocol';
import type { RpcTransport } from '../ports/transport';

/** What react-native-webview's ref, and the app's web twin of it, offers: run a script in the page. */
export type WebViewHost = { injectJavaScript(script: string): void };

export type WebViewHostTransport = RpcTransport & {
  /** The WebView's onMessage handler passes what the page posted through here. */
  receive(raw: string): void;
};

/** The script that hands one string to the page as a `message` event on its window. */
export function deliverScript(raw: string): string {
  return `window.dispatchEvent(new MessageEvent('message',{data:${JSON.stringify(raw)}}));true;`;
}

/** The app's side of a WebView: strings go down as injected scripts, and come up through `receive`. */
export function createWebViewHostTransport(host: () => WebViewHost | null | undefined): WebViewHostTransport {
  const listeners = new Set<(raw: string) => void>();
  return {
    send(raw) {
      const target = host();
      if (!target) throw new RpcError('closed', 'the WebView is not mounted');
      target.injectJavaScript(deliverScript(raw));
    },
    listen(onRaw) {
      listeners.add(onRaw);
      return () => {
        listeners.delete(onRaw);
      };
    },
    receive(raw) {
      for (const listener of [...listeners]) listener(raw);
    },
  };
}
