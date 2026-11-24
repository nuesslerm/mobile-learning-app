import { createEditorClient, toRemoteEngine } from '@lib/code-editor-api';
import { Logger } from '@lib/logging';
import { createRpcClient, createWebViewHostTransport, RpcError } from '@lib/webview-rpc';

import type { EditorBridgePort } from '../../../domain/ports';
import { editorSource } from './editorSource';
import { buildEditorHtml } from './editorHtml';

let page: string | undefined;

/** The CodeMirror page in a WebView, and the typed editor API over it: requests go down as injected scripts, replies and events come back as posted messages. */
export function createWebViewEditorBridge(): EditorBridgePort {
  return {
    html: () => page ??= buildEditorHtml({ profileInput: __DEV__ && process.env.EXPO_PUBLIC_EDITOR_PERF === '1' }),
    pageSource: editorSource,
    connect(webView) {
      const transport = createWebViewHostTransport(webView);
      const rpc = createRpcClient(transport, { timeoutMs: 5000 });
      const engine = toRemoteEngine(rpc, (error) => {
        // a listener that leaves after its page has gone finds the channel closed: that is the page going away, not a failure
        if (error instanceof RpcError && error.code === 'closed') Logger.debug('editor.api.channel_closed');
        else Logger.withError(error).warn('editor.api.remote_failed');
      });
      const client = createEditorClient(engine, { trace: (call) => Logger.debug('editor.api.call', { ...call }) });
      return { client, receive: (raw) => transport.receive(raw), close: () => rpc.close() };
    },
  };
}
