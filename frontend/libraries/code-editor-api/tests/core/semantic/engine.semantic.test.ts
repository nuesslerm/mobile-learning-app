import { describe, expect, it } from 'vitest';

import { applyChanges, createEditorClient, createEditorEngine, mapSelRange, unwrap, type EngineCore, type EditorEvent, type HostUpdate } from '../../../src';
import type { DocumentHost } from '../../../src/ports/host';
import { createFakeEditor, snapshotOf } from '../../../src/testing';

const BRANCH = 'def f(x):\n    if x:\n        a = 1\n        b = 2\n    return b\n';
const SECOND = { from: BRANCH.indexOf('b = 2'), to: BRANCH.indexOf('b = 2') + 5 };
const WITHOUT_SECOND = 'def f(x):\n    if x:\n        a = 1\n    return b\n';

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

// a parse that waits for the test: every turn it is given runs one more slice
function paced() {
  const releases: (() => void)[] = [];
  return {
    yieldToEvents: () => new Promise<void>((resolve) => releases.push(resolve)),
    release: () => releases.splice(0).forEach((resolve) => resolve()),
    waiting: () => releases.length,
  };
}

describe('a node step on the engine', () => {
  it('given a host with a tree and treeOf, when hello is read, then deleteNode is announced and no step the engine cannot plan; given a host without treeOf or without a tree, then none is', async () => {
    const withBoth = createFakeEditor({ text: BRANCH, tree: true });
    const noTreeOf = createEditorEngine(Object.assign({}, withBoth.host, { treeOf: undefined }) as DocumentHost, { instance: 'no-tree-of', logError: () => undefined });
    const noTree = createFakeEditor({ text: BRANCH });

    const steps = async (engine: EngineCore) => unwrap(await createEditorClient(engine).hello()).capabilities.steps;

    expect(await steps(withBoth.engine)).toEqual(['replace', 'move', 'select', 'type', 'complete', 'deleteNode']);
    expect(await steps(noTreeOf)).toEqual(['replace', 'move', 'select', 'type', 'complete']);
    expect(await steps(noTree.engine)).toEqual(['replace', 'move', 'select', 'type', 'complete']);
  });

  it('given a host without treeOf, when deleteNode is sent, then it is unsupported naming the step and nothing changes', async () => {
    const withBoth = createFakeEditor({ text: BRANCH, tree: true });
    const client = createEditorClient(createEditorEngine(Object.assign({}, withBoth.host, { treeOf: undefined }) as DocumentHost, { instance: 'no-tree-of', logError: () => undefined }));

    const reply = await client.deleteNode(SECOND);

    expect(reply).toMatchObject({ ok: false, error: { code: 'unsupported', what: 'step:deleteNode' } });
    expect(withBoth.text()).toBe(BRANCH);
  });

  it('given a step with no planner, when it is sent beside a deleteNode, then the whole call is unsupported naming that step and nothing changes', async () => {
    const fake = createFakeEditor({ text: BRANCH, tree: true });
    const client = createEditorClient(fake.engine);

    const reply = await client.transact([{ op: 'deleteNode', node: SECOND }, { op: 'moveNode', node: SECOND, to: { after: SECOND } }]);

    expect(reply).toMatchObject({ ok: false, error: { code: 'unsupported', what: 'step:moveNode' } });
    expect(fake.peek()).toMatchObject({ text: BRANCH, revision: 0, undoDepth: 0 });
  });

  it('given a deleteNode, when it is applied, then it is one revision, one changed event of origin api with its label, one undo step, and the reply names the base and a collapsed landed range', async () => {
    const fake = createFakeEditor({ text: BRANCH, tree: true });
    const client = createEditorClient(fake.engine);
    const events: EditorEvent[] = [];
    fake.engine.listen(['changed'], (event) => events.push(event));

    const reply = await client.deleteNode(SECOND, { label: 'drop' });

    const value = unwrap(reply);
    const from = BRANCH.indexOf('        b = 2');
    expect(reply).toMatchObject({ revision: 1, base: 0 });
    expect(value.changes).toEqual([{ from, to: from + '        b = 2\n'.length, insert: '' }]);
    expect(value.landed).toEqual([{ from, to: from }]);
    expect(fake.peek()).toMatchObject({ text: WITHOUT_SECOND, revision: 1, undoDepth: 1 });
    expect(events).toMatchObject([{ type: 'changed', from: 0, to: 1, origin: 'api', label: 'drop' }]);
    unwrap(await client.undo());
    expect(fake.text()).toBe(BRANCH);
  });

  it('given a document with no final newline, when its last statement is deleted, then the line before keeps its newline', async () => {
    const text = 'def f(x):\n    a = 1\n    b = 2';
    const fake = createFakeEditor({ text, tree: true });

    unwrap(await createEditorClient(fake.engine).deleteNode({ from: text.indexOf('b = 2'), to: text.length }));

    expect(fake.text()).toBe('def f(x):\n    a = 1\n');
  });

  it('given a call at an older revision, when it is sent with stale map, then it is stale and nothing changes', async () => {
    const fake = createFakeEditor({ text: BRANCH, tree: true });
    const client = createEditorClient(fake.engine);
    unwrap(await client.insertAt(0, '\n'));

    const reply = await client.deleteNode(SECOND, { at: 0, stale: 'map' });

    expect(reply).toMatchObject({ ok: false, error: { code: 'stale', head: 1 } });
    expect(fake.peek()).toMatchObject({ text: `\n${BRANCH}`, revision: 1 });
  });

  it('given a preview on a deleteNode, when it is sent, then the would-be text and its diff come back, nothing is applied, no event fires and no undo step exists', async () => {
    const fake = createFakeEditor({ text: BRANCH, tree: true });
    const client = createEditorClient(fake.engine);
    const events: EditorEvent[] = [];
    fake.engine.listen(['changed', 'selection'], (event) => events.push(event));

    const reply = await client.deleteNode(SECOND, { preview: true });

    const value = unwrap(reply);
    const from = BRANCH.indexOf('        b = 2');
    expect(value.changes).toEqual([]);
    expect(value.preview).toEqual({ text: WITHOUT_SECOND, diff: [{ from, to: from + '        b = 2\n'.length, insert: '' }], landed: [{ from, to: from }] });
    expect(reply).toMatchObject({ revision: 0, base: 0 });
    expect(fake.peek()).toMatchObject({ text: BRANCH, revision: 0, undoDepth: 0 });
    expect(events).toEqual([]);
  });

  it('given a preview on a raw replace, when it is sent, then the would-be text and its diff come back and the text is not edited', async () => {
    const fake = createFakeEditor({ text: 'abc' });

    const reply = await fake.engine.request('apply', { steps: [{ op: 'replace', range: { from: 0, to: 1 }, text: 'X' }], preview: true });

    expect(reply).toMatchObject({ ok: true, revision: 0, base: 0, value: { changes: [], landed: [{ from: 0, to: 1 }], preview: { text: 'Xbc', diff: [{ from: 0, to: 1, insert: 'X' }], landed: [{ from: 0, to: 1 }] } } });
    expect(fake.peek()).toMatchObject({ text: 'abc', revision: 0, undoDepth: 0 });
  });

  it('given a preview on a call that changes nothing, when it is sent, then the preview says so and the revision does not move', async () => {
    const fake = createFakeEditor({ text: 'abc' });

    const reply = await fake.engine.request('apply', { steps: [{ op: 'replace', range: { from: 1, to: 1 }, text: '' }], preview: true });

    expect(reply).toMatchObject({ ok: true, revision: 0, value: { changes: [], preview: { text: 'abc', diff: [] } } });
  });

  it('given a call with a preview and a type step, when it is sent, then it is invalid', async () => {
    const fake = createFakeEditor({ text: 'abc' });

    const reply = await fake.engine.request('apply', { steps: [{ op: 'type', text: 'x' }], preview: true });

    expect(reply).toMatchObject({ ok: false, error: { code: 'invalid', path: 'preview' } });
  });
});

describe('a node step while the parse waits for a turn', () => {
  const slow = (turns: number, options: { now?: () => number } = {}) => {
    const parse = paced();
    const fake = createFakeEditor({ text: BRANCH, tree: true, engine: { yieldToEvents: parse.yieldToEvents, now: options.now ?? (() => 0), treeSliceMs: 1 } });
    fake.slowParse?.(turns);
    return { fake, parse, client: createEditorClient(fake.engine) };
  };

  it('given a parse waiting for a turn and a keystroke typed meanwhile, when the call resumes, then it is structure-changed with the head and nothing is applied', async () => {
    const { fake, parse, client } = slow(4);
    const pending = client.deleteNode(SECOND);
    await settle();
    expect(parse.waiting()).toBe(1);

    fake.userTypes('#');
    parse.release();
    const reply = await pending;

    expect(reply).toMatchObject({ ok: false, revision: 1, error: { code: 'structure-changed', head: 1 } });
    expect(fake.text()).toBe(`#${BRANCH}`);
    expect(fake.peek().undoDepth).toBe(1);
  });

  it('given an apply sent after a waiting node step, when the parse is released, then the apply is answered first and the node step then says structure-changed', async () => {
    const { fake, parse, client } = slow(4);
    const order: string[] = [];
    const pending = client.deleteNode(SECOND).then((reply) => (order.push('node step'), reply));
    await settle();

    const applied = client.insertAt(0, '\n').then((reply) => (order.push('apply'), reply));
    await settle();
    parse.release();
    const reply = await pending;
    unwrap(await applied);

    expect(order).toEqual(['apply', 'node step']);
    expect(reply).toMatchObject({ ok: false, error: { code: 'structure-changed', head: 1 } });
    expect(fake.text()).toBe(`\n${BRANCH}`);
  });

  it('given a base parse that does not finish within the budget, when the call runs, then it is parse-timeout naming the budget and nothing is applied', async () => {
    let clock = 0;
    const { fake, parse, client } = slow(1000, { now: () => (clock += 10) });
    const pending = client.deleteNode(SECOND, { budgetMs: 50 });
    for (let turn = 0; turn < 10; turn += 1) {
      await settle();
      parse.release();
    }

    const reply = await pending;

    expect(reply).toMatchObject({ ok: false, error: { code: 'parse-timeout', budgetMs: 50 } });
    expect(fake.peek()).toMatchObject({ text: BRANCH, revision: 0 });
  });

  it('given a result parse that does not finish within the budget, when the call runs, then it is parse-timeout and nothing is committed', async () => {
    let clock = 0;
    const fake = createFakeEditor({
      text: BRANCH,
      tree: (text, language) => snapshotOf(text, language, undefined, text === BRANCH ? 1 : 1000),
      engine: { now: () => (clock += 10), yieldToEvents: async () => undefined, treeSliceMs: 1 },
    });

    const reply = await createEditorClient(fake.engine).deleteNode(SECOND, { budgetMs: 50 });

    expect(reply).toMatchObject({ ok: false, error: { code: 'parse-timeout', budgetMs: 50 } });
    expect(fake.peek()).toMatchObject({ text: BRANCH, revision: 0, undoDepth: 0 });
  });
});

// a host whose page has typed something the engine has not been told about: it is delivered on the next sync, as a page whose update listener failed delivers it
function lagging(text: string, options: { readonly typeDuringResultParse?: string } = {}) {
  let doc = text;
  let caret = { anchor: 0, head: 0 };
  let revision = 0;
  let commits = 0;
  const undelivered: HostUpdate[] = [];
  const holder: { core: EngineCore | null } = { core: null };
  const keystroke = (insert: string) => {
    doc = insert + doc;
    revision += 1;
    undelivered.push({ revision, changes: [{ from: 0, to: 0, insert }], selection: { ranges: [caret], main: 0 }, origin: 'user', length: doc.length });
  };
  const host: DocumentHost = {
    doc: { get length() { return doc.length; }, slice: (from, to) => doc.slice(from, to) },
    selection: () => ({ ranges: [caret], main: 0 }),
    hasFocus: () => false,
    sync() {
      for (const update of undelivered.splice(0)) holder.core?.ingest(update);
    },
    tree: () => snapshotOf(doc, 'python', undefined, 3),
    treeOf: (other) => {
      const inner = snapshotOf(other, 'python');
      return {
        ...inner,
        work(upto, ms) {
          if (options.typeDuringResultParse) keystroke(options.typeDuringResultParse);
          return inner.work(upto, ms);
        },
      };
    },
    commit({ changes, selection }) {
      commits += 1;
      doc = applyChanges(doc, changes);
      caret = selection ?? mapSelRange(caret, changes);
      revision += 1;
      undelivered.push({ revision, changes, selection: { ranges: [caret], main: 0 }, origin: 'api', length: doc.length });
    },
    history: () => false,
    load: () => ({ text: doc, selection: { ranges: [caret], main: 0 }, revision }),
    configure: () => ({ ok: false, error: { code: 'internal', message: 'not under test' } }),
    setFocus: () => undefined,
    languages: () => ['python'],
    maxSelections: 1,
  };
  const turns = paced();
  holder.core = createEditorEngine(host, { instance: 'lagging', logError: () => undefined, now: () => 0, yieldToEvents: turns.yieldToEvents, treeSliceMs: 1 });
  return { client: createEditorClient(holder.core), keystroke, turns, text: () => doc, commits: () => commits };
}

describe('a node step on a host that delivers its transactions late', () => {
  it('given a keystroke the host has not delivered when the base parse ends, when the call resumes, then it is structure-changed and nothing is committed', async () => {
    const page = lagging(BRANCH);
    const pending = page.client.deleteNode(SECOND);
    await settle();

    page.keystroke('#');
    for (let turn = 0; turn < 3; turn += 1) {
      page.turns.release();
      await settle();
    }
    const reply = await pending;

    expect(reply).toMatchObject({ ok: false, error: { code: 'structure-changed', head: 1 } });
    expect(page.commits()).toBe(0);
    expect(page.text()).toBe(`#${BRANCH}`);
  });

  it('given a keystroke the host has not delivered when the result parse ends, when the call resumes, then it is structure-changed and nothing is committed', async () => {
    const page = lagging(BRANCH, { typeDuringResultParse: '#' });
    const pending = page.client.deleteNode(SECOND);
    for (let turn = 0; turn < 3; turn += 1) {
      await settle();
      page.turns.release();
    }

    const reply = await pending;

    expect(reply).toMatchObject({ ok: false, error: { code: 'structure-changed', head: 1 } });
    expect(page.commits()).toBe(0);
    expect(page.text()).toBe(`#${BRANCH}`);
  });
});
