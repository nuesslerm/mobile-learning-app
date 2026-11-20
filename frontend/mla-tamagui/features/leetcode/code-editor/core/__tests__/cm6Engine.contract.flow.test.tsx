import { createRef } from 'react';
import { editorContract } from '@lib/code-editor-api/testing';
import { toRemoteEngine } from '@lib/code-editor-api';
import { act, FlowProviders, waitFor } from '@lib/flow-test-harness';
import { createRpcClient, createWebViewHostTransport, decode, deliverScript, type RpcTransport } from '@lib/webview-rpc';
import { createRoot } from 'react-dom/client';
import { WebView } from 'react-native-webview';
import { afterAll, vi } from 'vitest';

import { buildEditorHtml } from '../editorHtml';

/**
 * The typed editor API on the real committed CodeMirror bundle, through the app's WebView twin: the same suite the fake passes over rpc.
 * Not provable here: typing through the DOM and IME (the Chromium editor-typing e2e), soft-keyboard focus, layout, and Android bridge timing.
 */
vi.setConfig({ testTimeout: 30_000, hookTimeout: 30_000 });

const INIT = {
  type: 'init',
  config: {
    initialText: '',
    isDark: false,
    fontSize: 14,
    lineHeight: 20,
    wordWrap: false,
    language: 'python',
    cursorColor: '#000000',
    selectionColor: '#cccccc',
    backgroundColor: '#ffffff',
    gutterColor: '#eeeeee',
    borderColor: '#dddddd',
    tabSize: 4,
  },
};

type DebugWindow = Window & { __editorDebug__?: { simulateUser(spec: { kind: 'type'; text: string } | { kind: 'select'; anchor: number; head?: number }): void } };

// mounted by hand, not through renderScreen: testing-library unmounts what it rendered after every test, and the suite shares one page per file
const mounted: { unmount(): void; container: HTMLElement }[] = [];
afterAll(async () => {
  for (const { unmount, container } of mounted) {
    await act(async () => unmount());
    container.remove();
  }
});

async function mountPage(options: { init: boolean }) {
  // the flow config aliases react-native-webview to the app's web twin, whose handle injects scripts into the iframe
  const ref = createRef<{ injectJavaScript(script: string): void }>();
  const host = createWebViewHostTransport(() => ref.current);
  // the failures a real channel has: a lost reply, a lost event, a page that stops hearing requests
  const faults = { dropReply: false, dropEvent: false, silent: false };
  const client: RpcTransport = {
    send: (raw) => {
      const decoded = decode(raw);
      if (faults.silent && decoded.type === 'message' && decoded.message.kind === 'req') return;
      host.send(raw);
    },
    listen: (onRaw) => host.listen(onRaw),
  };
  const rpc = createRpcClient(client, { timeoutMs: 2000 });
  const errors: unknown[] = [];
  const crossed: string[] = [];
  const sendInit = () => ref.current?.injectJavaScript(deliverScript(JSON.stringify(INIT)));
  let announced = false;
  const container = document.body.appendChild(document.createElement('div'));
  const root = createRoot(container);
  mounted.push({ unmount: () => root.unmount(), container });
  await act(async () => {
    root.render(
      <FlowProviders>
        <WebView
          ref={ref as never}
          source={{ html: buildEditorHtml({ debug: true }) }}
          onMessage={(event: { nativeEvent: { data: string } }) => {
            const raw = event.nativeEvent.data;
            const decoded = decode(raw);
            if (decoded.type === 'message' && decoded.message.kind === 'res' && faults.dropReply) {
              faults.dropReply = false;
              return;
            }
            if (decoded.type === 'message' && decoded.message.kind === 'evt' && decoded.message.topic === 'editor.event') {
              const type = (decoded.message.data as { type?: string } | undefined)?.type;
              if (type === 'changed' && faults.dropEvent) {
                faults.dropEvent = false;
                return;
              }
              if (type) crossed.push(type);
            }
            host.receive(raw);
            if (decoded.type === 'foreign' && (JSON.parse(raw) as { type?: string }).type === 'ready' && !announced) {
              announced = true;
              if (options.init) sendInit();
            }
          }}
        />
      </FlowProviders>
    );
  });
  const engine = toRemoteEngine(rpc, (error) => errors.push(error));
  const frame = () => container.querySelector('iframe')?.contentWindow as DebugWindow | null | undefined;
  await waitFor(() => {
    if (!announced) throw new Error('the page has not announced itself');
  });
  return { engine, rpc, sendInit, frame, crossed, errors, faults };
}

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
});
