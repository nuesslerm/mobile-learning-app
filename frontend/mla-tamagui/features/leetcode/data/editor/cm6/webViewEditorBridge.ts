import { createEditorClient, toRemoteEngine } from '@lib/code-editor-api';
import { Logger } from '@lib/logging';
import { createRpcClient, createWebViewHostTransport, RpcError } from '@lib/webview-rpc';

import type { EditorBridgePort } from '../../../domain/ports';
import { buildEditorHtml } from './editorHtml';

/** The CodeMirror page in a WebView, and the typed editor API over it: requests go down as injected scripts, replies and events come back as posted messages. */
export function createWebViewEditorBridge(): EditorBridgePort {
  return {
    html: () => buildEditorHtml(),
    connect(webView) {
      const transport = createWebViewHostTransport(webView);
      const engine = toRemoteEngine(createRpcClient(transport, { timeoutMs: 5000 }), (error) => {
        // a listener that leaves after its page has gone finds the channel closed: that is the page going away, not a failure
        if (error instanceof RpcError && error.code === 'closed') Logger.debug('editor.api.channel_closed');
        else Logger.withError(error).warn('editor.api.remote_failed');
      });
      const client = createEditorClient(engine, { trace: (call) => Logger.debug('editor.api.call', { ...call }) });
      return { client, receive: (raw) => transport.receive(raw) };
    },
  };
}
