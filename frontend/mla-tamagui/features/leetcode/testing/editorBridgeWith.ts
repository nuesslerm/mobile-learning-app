import type { EditorClient } from '@lib/code-editor-api';

import { createWebViewEditorBridge } from '../data/editor/cm6/webViewEditorBridge';
import type { EditorBridgePort } from '../domain/ports';

/** A bridge whose client answers the calls `change` names itself, given the page's own client; every other call is the page's. */
export function editorBridgeWith(change: (real: EditorClient) => Partial<EditorClient>): EditorBridgePort {
  const real = createWebViewEditorBridge();
  return {
    html: () => real.html(),
    connect: (webView) => {
      const bridge = real.connect(webView);
      return { receive: bridge.receive, client: { ...bridge.client, ...change(bridge.client) } };
    },
  };
}
