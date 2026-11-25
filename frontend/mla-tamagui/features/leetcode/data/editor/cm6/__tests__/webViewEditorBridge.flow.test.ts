import { expectWarning } from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import { forgetLoggedLines, loggedLines } from '../../../../../../tests/flow/loggedLines';
import { createForwardingPost } from '../../../../domain/editor/editorForwarding';
import { createWebViewEditorBridge } from '../webViewEditorBridge';

describe('the WebView editor bridge', () => {
  it('given a page that has gone, when a listener of the client leaves, then the closed channel is logged as such and is no warning', async () => {
    await forgetLoggedLines();
    const { client } = createWebViewEditorBridge().connect(() => null);

    const leave = client.on('changed', () => undefined);
    leave();

    expect((await loggedLines('editor.api.channel_closed')).length).toBeGreaterThan(0);
    expect(await loggedLines('editor.api.remote_failed')).toEqual([]);
  });

  it('given a pending editor subscription, when the page closes, then the request ends as an ordinary channel close', async () => {
    await forgetLoggedLines();
    const bridge = createWebViewEditorBridge().connect(() => ({ injectJavaScript: () => undefined }));
    bridge.client.on('changed', () => undefined);

    bridge.close();

    expect((await loggedLines('editor.api.channel_closed')).length).toBeGreaterThan(0);
    expect(await loggedLines('editor.api.remote_failed')).toEqual([]);
  });

  it.each([true, false])('given a settings request in flight, when the bridge closes with disposal %s, then only an unexpected close warns', async (disposal) => {
    await forgetLoggedLines();
    const bridge = createWebViewEditorBridge().connect(() => ({ injectJavaScript: () => undefined }));
    let disposing = false;
    const post = createForwardingPost(bridge.client, () => undefined, {
      blurThroughApi: true,
      isDisposing: () => disposing,
    });
    if (!disposal) expectWarning(/editor\.port\.forward_failed/);

    post({ type: 'updateConfig', config: { wordWrap: true } });
    disposing = disposal;
    bridge.close();

    const warnings = await loggedLines('editor.port.forward_failed');
    expect(warnings).toHaveLength(disposal ? 0 : 1);
  });
});
