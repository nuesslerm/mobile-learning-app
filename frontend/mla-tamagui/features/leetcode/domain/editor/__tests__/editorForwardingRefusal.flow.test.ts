import { createEditorClient } from '@lib/code-editor-api';
import { createFakeEditor } from '@lib/code-editor-api/testing';
import { expectWarning } from '@lib/flow-test-harness';
import { describe, expect, it } from 'vitest';

import { forgetLoggedLines, loggedLines } from '../../../../../tests/flow/loggedLines';
import type { ToEditorMessage } from '../../ports';
import { createForwardingPost } from '../editorForwarding';

describe('the editor port forwarding, as logged', () => {
  it('given an updateConfig with a key the typed API does not carry, when it is posted, then the refusal is logged once with the message type and a code', async () => {
    // Given a forwarding port over an editor, and a log with nothing in it
    await forgetLoggedLines();
    const legacy: ToEditorMessage[] = [];
    const post = createForwardingPost(createEditorClient(createFakeEditor({ text: 'abc' }).engine), (msg) => legacy.push(msg));
    expectWarning(/editor\.port\.forward_failed/);

    // When a patch with a key the API does not carry is posted
    post({ type: 'updateConfig', config: { initialText: 'x', wordWrap: true } });

    // Then one line says which message was refused and why, and nothing took the old script path
    const lines = await loggedLines('editor.port.forward_failed');
    expect(lines).toHaveLength(1);
    expect(lines[0]?.context).toMatchObject({ type: 'updateConfig', code: 'unmapped-config' });
    expect(legacy).toEqual([]);
  });
});
