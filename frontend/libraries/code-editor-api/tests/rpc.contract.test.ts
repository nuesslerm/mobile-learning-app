import { createRpcClient } from '@lib/webview-rpc';
import { createEmulatedWebView, createLoopbackPair } from '@lib/webview-rpc/testing';

import { createDeferredEngine, LANGUAGE_NAMES, serveEditor, toRemoteEngine } from '../src';
import { createFakeEditor, editorContract, withFaults, type FakeEditor } from '../src/testing';

function overRpc(name: string, makePair: typeof createLoopbackPair) {
  const errors: unknown[] = [];
  editorContract(name, async ({ bound }) => {
    const faulty = withFaults(makePair());
    const deferred = createDeferredEngine((error) => errors.push(error));
    let generation = 0;
    let current: FakeEditor | null = null;
    const bind = () => {
      generation += 1;
      current = createFakeEditor({ instance: `fake-${generation}`, text: '' });
      deferred.bind(current.engine);
    };
    if (bound) bind();
    const server = serveEditor(deferred, faulty.page, (error) => errors.push(error));
    const rpc = createRpcClient(faulty.host, { timeoutMs: 300 });
    return {
      engine: toRemoteEngine(rpc, (error) => errors.push(error)),
      dispose: () => {
        rpc.close();
        server.close();
      },
      hooks: {
        userTypes: (text) => current?.userTypes(text),
        userSelects: (anchor, head) => current?.userSelects(anchor, head),
        restart: bind,
        ...faulty.controls,
      },
    };
  }, { languages: LANGUAGE_NAMES });
}

overRpc('the fake editor over a loopback channel', createLoopbackPair);
overRpc('the fake editor over an emulated WebView', createEmulatedWebView);
