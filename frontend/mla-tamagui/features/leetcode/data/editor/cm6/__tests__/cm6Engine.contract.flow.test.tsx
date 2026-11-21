import { LANGUAGE_NAMES } from '@lib/code-editor-api';
import { editorContract, semanticContract, syntaxTreeContract, type MakeSubject } from '@lib/code-editor-api/testing';
import { waitFor } from '@lib/flow-test-harness';
import { vi } from 'vitest';

import { mountPage } from './support/mountEditorPage';

/**
 * The typed editor API on the real committed CodeMirror bundle, through the app's WebView twin: the same suite the fake passes over rpc.
 * Not provable here: typing through the DOM and IME (the Chromium editor-typing e2e), soft-keyboard focus, layout, and Android bridge timing.
 */
vi.setConfig({ testTimeout: 30_000, hookTimeout: 30_000 });

const makeSubject: MakeSubject = async ({ bound }) => {
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
};

editorContract('the real CodeMirror bundle in the WebView twin', makeSubject, { languages: LANGUAGE_NAMES });

// one page serves every language: the suite switches it with `configure({language})`, as the screen does
for (const language of LANGUAGE_NAMES) syntaxTreeContract('the real CodeMirror bundle in the WebView twin', makeSubject, language);
for (const language of LANGUAGE_NAMES) semanticContract('the real CodeMirror bundle in the WebView twin', makeSubject, language);
