import { RpcError } from '../core/protocol';
import type { RpcTransport } from '../ports/transport';

type PageMessageListener = (event: { data: unknown }) => void;

/** The part of the page's `window` the transport uses; `ReactNativeWebView` is what both the native WebView and the web twin inject. */
export type PageWindow = {
  ReactNativeWebView?: { postMessage(data: string): void };
  addEventListener(type: 'message', listener: PageMessageListener): void;
  removeEventListener(type: 'message', listener: PageMessageListener): void;
};

/** The page's side of a WebView: strings go up through `ReactNativeWebView.postMessage`, and come down as `message` events. */
export function createPageTransport(win: PageWindow): RpcTransport {
  return {
    send(raw) {
      const bridge = win.ReactNativeWebView;
      if (!bridge) throw new RpcError('closed', 'the page has no ReactNativeWebView bridge');
      bridge.postMessage(raw);
    },
    listen(onRaw) {
      const listener: PageMessageListener = (event) => {
        if (typeof event.data === 'string') onRaw(event.data);
      };
      win.addEventListener('message', listener);
      return () => win.removeEventListener('message', listener);
    },
  };
}
