import { LANGUAGE_NAMES } from '@lib/code-editor-api';
import { editorContract } from '@lib/code-editor-api/testing';
import { waitFor } from '@lib/flow-test-harness';
import { vi } from 'vitest';

import { mountPage } from './support/mountEditorPage';

/**
 * The typed editor API on the real committed CodeMirror bundle, through the app's WebView twin: the same suite the fake passes over rpc.
 * Not provable here: typing through the DOM and IME (the Chromium editor-typing e2e), soft-keyboard focus, layout, and Android bridge timing.
 */
vi.setConfig({ testTimeout: 30_000, hookTimeout: 30_000 });

editorContract('the real CodeMirror bundle in the WebView twin', async ({ bound }) => {
  const page = await mountPage({ init: bound });
  if (bound) {
    await waitFor(async () => {
      const reply = await page.engine.request('hello', undefined);
      if (!reply.ok || !reply.value.ready) throw new Error('the editor is not ready');
    });
  }
  return {
    engine: page.engine,
    dispose: () => page.rpc.close(),
    hooks: {
      userTypes: (text) => page.frame()?.__editorDebug__?.simulateUser({ kind: 'type', text }),
      userSelects: (anchor, head) => page.frame()?.__editorDebug__?.simulateUser({ kind: 'select', anchor, ...(head === undefined ? {} : { head }) }),
      restart: () => page.sendInit(),
      closeTransport: () => page.rpc.close(),
      dropNextReply: () => {
        page.faults.dropReply = true;
      },
      swallowNextEvent: () => {
        page.faults.dropEvent = true;
      },
      silentPage: () => {
        page.faults.silent = true;
        return () => {
          page.faults.silent = false;
        };
      },
      transportEvents: () => page.crossed,
    },
  };
}, { languages: LANGUAGE_NAMES });
