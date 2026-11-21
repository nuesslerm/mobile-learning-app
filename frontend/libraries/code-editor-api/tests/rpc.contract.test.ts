import { createRpcClient } from '@lib/webview-rpc';
import { createEmulatedWebView, createLoopbackPair } from '@lib/webview-rpc/testing';

import { createDeferredEngine, LANGUAGE_NAMES, serveEditor, toRemoteEngine } from '../src';
import { createFakeEditor, editorContract, semanticContract, syntaxTreeContract, withFaults, type FakeEditor } from '../src/testing';
import { lezerSnapshot } from './support/lezerPlain';

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

  syntaxTreeContract(name, async ({ bound }) => {
    const faulty = withFaults(makePair());
    const deferred = createDeferredEngine((error) => errors.push(error));
    let current: FakeEditor | null = null;
    const bind = () => {
      current = createFakeEditor({ instance: 'fake-tree', text: '', tree: true });
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
      hooks: { restart: bind, slowParse: (calls) => current?.slowParse?.(calls) },
    };
  }, 'python');

  // a node step is a deferred reply: it crosses the channel like a tree's, over the grammars' own trees
  for (const language of ['python', 'javascript'] as const) {
    semanticContract(name, async ({ bound }) => {
      const faulty = withFaults(makePair());
      const deferred = createDeferredEngine((error) => errors.push(error));
      if (bound) deferred.bind(createFakeEditor({ instance: 'fake-semantic', text: '', tree: lezerSnapshot }).engine);
      const server = serveEditor(deferred, faulty.page, (error) => errors.push(error));
      const rpc = createRpcClient(faulty.host, { timeoutMs: 2000 });
      return {
        engine: toRemoteEngine(rpc, (error) => errors.push(error)),
        dispose: () => {
          rpc.close();
          server.close();
        },
        hooks: {},
      };
    }, language);
  }
}

overRpc('the fake editor over a loopback channel', createLoopbackPair);
overRpc('the fake editor over an emulated WebView', createEmulatedWebView);
