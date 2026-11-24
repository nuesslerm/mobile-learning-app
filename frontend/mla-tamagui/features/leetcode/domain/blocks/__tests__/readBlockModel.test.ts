import { createEditorClient, unwrap, type CallTrace, type EditorClient, type Language, type Reply, type TreeSnapshot } from '@lib/code-editor-api';
import { createFakeEditor, parseFor, snapshotOf } from '@lib/code-editor-api/testing';
import { describe, expect, it } from 'vitest';

import { MAX_TREE_ASKS, readBlockModel } from '../readBlockModel';

const DEF_IF = ['def f(x):', '    if x:', '        a = 1', '        b = 2', '    return b', ''].join('\n');

function editor(text: string, options: { instance?: string } = {}) {
  const calls: CallTrace[] = [];
  const fake = createFakeEditor({ text, tree: true, ...options });
  const client = createEditorClient(fake.engine, { trace: (call) => calls.push(call) });
  return { fake, client, calls };
}

/** An editor whose parse needs `turns` slices, one per ask, as a real page's does: the snapshot of a text is kept between asks. */
function slowEditor(text: string, turns: number) {
  const calls: CallTrace[] = [];
  const kept = new Map<string, TreeSnapshot>();
  let time = 0;
  const fake = createFakeEditor({
    text,
    tree: (source: string, language: Language) => {
      const snapshot = kept.get(`${language}:${source}`) ?? snapshotOf(source, language, parseFor, turns);
      kept.set(`${language}:${source}`, snapshot);
      return snapshot;
    },
    // a clock that passes the budget at every look: each ask gets exactly one slice of parsing
    engine: { now: () => (time += 100) },
  });
  return { fake, calls, client: createEditorClient(fake.engine, { trace: (call) => calls.push(call) }) };
}

const treeAsks = (calls: readonly CallTrace[]) => calls.filter((call) => call.name === 'tree').length;

describe('readBlockModel', () => {
  it('given an editor holding the def-and-if example, when the model is read, then it is the rows of that text and its revision is the tree reply\'s base', async () => {
    const { client } = editor(DEF_IF);

    const read = await readBlockModel(client);
    const tree = await client.tree({ detail: 'outline' });

    expect(read.kind).toBe('ready');
    if (read.kind !== 'ready' || !tree.ok) throw new Error('not ready');
    expect(read.model.revision).toBe(tree.base);
    expect(read.model.instance).toBe(tree.value.instance);
    expect(read.model.text).toBe(DEF_IF);
    expect(read.model.rows.map((row) => row.label)).toEqual(['def f(x):', 'if x:', 'a = 1', 'b = 2', 'return b']);
    expect(read.model.complete).toBe(true);
  });

  it('given a keystroke between the text read and the tree read, when the model is read, then the two are read again and the model is of the typed text', async () => {
    const { fake, client } = editor(DEF_IF);
    let typed = false;
    const typing: EditorClient = {
      ...client,
      tree: async (params) => {
        if (!typed) {
          typed = true;
          fake.userTypes('# ');
        }
        return client.tree(params);
      },
    };

    const read = await readBlockModel(typing);

    expect(read.kind).toBe('ready');
    if (read.kind !== 'ready') throw new Error('not ready');
    expect(read.model.text).toBe(`# ${DEF_IF}`);
    expect(read.model.revision).toBe(fake.peek().revision);
  });

  it('given a keystroke between the two reads every time, when the model is read, then it is stale after the one retry', async () => {
    const { fake, client, calls } = editor(DEF_IF);
    const typing: EditorClient = {
      ...client,
      tree: async (params) => {
        fake.userTypes('#');
        return client.tree(params);
      },
    };

    const read = await readBlockModel(typing);

    expect(read).toEqual({ kind: 'stale' });
    expect(calls.filter((call) => call.name === 'read').length).toBe(2);
  });

  it('given a parse that needs three asks, when the model is read, then the tree is asked for again until it is complete', async () => {
    const { client, calls } = slowEditor(DEF_IF, 3);

    const read = await readBlockModel(client);

    expect(read.kind).toBe('ready');
    if (read.kind !== 'ready') throw new Error('not ready');
    expect(read.model.complete).toBe(true);
    expect(treeAsks(calls)).toBe(3);
    expect(read.model.rows).toHaveLength(5);
  });

  it('given a parse that never completes, when the model is read, then the asks stop at the bound and the model is drawn as far as it reached', async () => {
    const { client, calls } = slowEditor(DEF_IF, 100_000);

    const read = await readBlockModel(client);

    expect(treeAsks(calls)).toBe(MAX_TREE_ASKS);
    expect(read.kind).toBe('ready');
    if (read.kind !== 'ready') throw new Error('not ready');
    expect(read.model.complete).toBe(false);
  });

  it('given an editor switched to JavaScript, when the model is read, then it is unsupported for that language', async () => {
    const { client } = editor('let a = 1;\n');
    unwrap(await client.setConfig({ language: 'javascript' }));

    expect(await readBlockModel(client)).toEqual({ kind: 'unsupported', language: 'javascript' });
  });

  it('given an editor with no tree, when the model is read, then it is unavailable', async () => {
    const client = createEditorClient(createFakeEditor({ text: DEF_IF }).engine);

    expect(await readBlockModel(client)).toEqual({ kind: 'unavailable' });
  });

  it('given an editor that says it is not ready, when the model is read, then it is not ready and no text is read', async () => {
    const { client, calls } = editor(DEF_IF);
    const sleeping: EditorClient = {
      ...client,
      hello: async () => {
        const reply = await client.hello();
        return reply.ok ? { ...reply, value: { ...reply.value, ready: false } } : reply;
      },
    };

    expect(await readBlockModel(sleeping)).toEqual({ kind: 'not-ready' });
    expect(calls.some((call) => call.name === 'read')).toBe(false);
  });

  it('given a page that stops answering the text read, when the model is read, then it fails with the transport code', async () => {
    const { client } = editor(DEF_IF);
    const lost: Reply<never> = { ok: false, revision: null, error: { code: 'transport', message: 'no answer', cause: 'timeout' } };
    const silent: EditorClient = { ...client, getText: async () => lost };

    expect(await readBlockModel(silent)).toEqual({ kind: 'failed', code: 'transport' });
  });
});
