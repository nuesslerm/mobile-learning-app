/** The real committed CodeMirror bundle in the app's WebView twin, with the faults a real channel has; shared by the flow tests that need the page itself. */
import { createRef } from 'react';
import { toRemoteEngine } from '@lib/code-editor-api';
import { act, FlowProviders, waitFor } from '@lib/flow-test-harness';
import { createRpcClient, createWebViewHostTransport, decode, deliverScript, type RpcTransport } from '@lib/webview-rpc';
import { createRoot } from 'react-dom/client';
import { WebView } from 'react-native-webview';
import { afterAll } from 'vitest';

import { buildEditorHtml } from '../../editorHtml';

export const INIT = {
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

export type DebugWindow = Window & { __editorDebug__?: { simulateUser(spec: { kind: 'type'; text: string } | { kind: 'select'; anchor: number; head?: number }): void } };

// mounted by hand, not through renderScreen: testing-library unmounts what it rendered after every test, and the suite shares one page per file
const mounted: { unmount(): void; container: HTMLElement }[] = [];
afterAll(async () => {
  for (const { unmount, container } of mounted) {
    await act(async () => unmount());
    container.remove();
  }
});

export async function mountPage(options: { init: boolean; language?: string }) {
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
  const readyMessages: unknown[] = [];
  const sendInit = () => ref.current?.injectJavaScript(deliverScript(JSON.stringify({ ...INIT, config: { ...INIT.config, language: options.language ?? INIT.config.language } })));
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
            if (decoded.type === 'foreign' && (JSON.parse(raw) as { type?: string }).type === 'ready') readyMessages.push(JSON.parse(raw));
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
  return { engine, rpc, sendInit, frame, crossed, errors, faults, readyMessages };
}

