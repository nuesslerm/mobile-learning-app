import { describe, expect, it } from 'vitest';

import { forgetLoggedLines, loggedLines } from '../../../../../../tests/flow/loggedLines';
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
});
