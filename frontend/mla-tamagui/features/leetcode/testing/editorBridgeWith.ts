import type { ApplyValue, EditorClient, Reply, Step } from '@lib/code-editor-api';

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

type Transact = EditorClient['transact'];

/** One `transact` call the page's client made, and what it was answered. */
export type RecordedTransact = { readonly steps: readonly Step[]; readonly options: Parameters<Transact>[1]; readonly reply: Reply<ApplyValue> };

export type RecordingEditorBridge = {
  readonly bridge: EditorBridgePort;
  /** every `transact` call so far, in order */
  readonly transacts: readonly RecordedTransact[];
  /** from now on a tree read waits; the returned function lets it, and every read after, through */
  holdTrees(): () => void;
};

/**
 * The page's own bridge, recording each `transact` call with its reply. `answer` may reply to a call in the page's place (a refusal the page would not give); a tree read can be held until
 * a test lets it through, so a test controls when a client sees the text it has just changed.
 */
export function recordingEditorBridge(options: { answer?: (steps: readonly Step[]) => Reply<ApplyValue> | null } = {}): RecordingEditorBridge {
  const transacts: RecordedTransact[] = [];
  let gate: Promise<void> | null = null;
  const bridge = editorBridgeWith((real) => ({
    transact: async (steps, callOptions) => {
      const reply = options.answer?.(steps) ?? (await real.transact(steps, callOptions));
      transacts.push({ steps, options: callOptions, reply });
      return reply;
    },
    tree: async (params) => {
      if (gate) await gate;
      return real.tree(params);
    },
  }));
  return {
    bridge,
    transacts,
    holdTrees() {
      let open!: () => void;
      gate = new Promise<void>((resolve) => {
        open = () => {
          gate = null;
          resolve();
        };
      });
      return open;
    },
  };
}
