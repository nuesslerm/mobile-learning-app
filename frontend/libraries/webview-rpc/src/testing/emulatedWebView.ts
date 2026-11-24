import { createPageTransport, type PageWindow } from '../adapters/webviewPage';
import { createWebViewHostTransport } from '../adapters/webviewHost';
import type { RpcTransport } from '../ports/transport';

type Runtime = {
  EventTarget: new () => { addEventListener: PageWindow['addEventListener']; removeEventListener: PageWindow['removeEventListener']; dispatchEvent(event: unknown): boolean };
  MessageEvent: new (type: string, init: { data: unknown }) => unknown;
};

const later = (fn: () => void) => void Promise.resolve().then(fn);

/**
 * The real host and page transports over an emulated WebView: the host's injected script is run against a fake page window
 * (so the script text is exercised, not skipped), and the page's `ReactNativeWebView.postMessage` reaches the host's `receive`.
 */
export function createEmulatedWebView(): { host: RpcTransport; page: RpcTransport } {
  const runtime = globalThis as unknown as Runtime;
  const win = new runtime.EventTarget() as unknown as PageWindow & { dispatchEvent(event: unknown): boolean };
  const host = createWebViewHostTransport(() => ({
    injectJavaScript: (script) => later(() => new Function('window', 'MessageEvent', script)(win, runtime.MessageEvent)),
  }));
  win.ReactNativeWebView = { postMessage: (data) => later(() => host.receive(String(data))) };
  return { host, page: createPageTransport(win) };
}
