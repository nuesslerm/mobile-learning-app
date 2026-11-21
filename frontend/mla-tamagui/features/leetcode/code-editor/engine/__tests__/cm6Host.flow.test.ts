import { closeBrackets } from '@codemirror/autocomplete';
import { history, undo } from '@codemirror/commands';
import { cpp } from '@codemirror/lang-cpp';
import { python } from '@codemirror/lang-python';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { classify, createDocumentMirror, createEditorEngine, enclosingBlock, unwrap, type ApplyParams, type EditorEvent, type EngineCore, type EngineOptions, type Language } from '@lib/code-editor-api';
import { afterEach, describe, expect, it } from 'vitest';

import { createCm6Host, revisionField, setRevisionBase, typingCalls } from '../cm6Host';

// A real EditorView in jsdom whose own update listener is broken: the host's pump must not depend on it.
const views: EditorView[] = [];
afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
});

function mount(doc: string, options: { language?: Language; engine?: Pick<EngineOptions, 'now' | 'yieldToEvents' | 'treeSliceMs'> } = {}) {
  const language = options.language ?? 'python';
  setRevisionBase(0);
  const swallowed: unknown[] = [];
  const extensions = [language === 'cpp' ? cpp() : python(), history(), closeBrackets(), revisionField, typingCalls, EditorView.exceptionSink.of((error) => swallowed.push(error)), EditorView.updateListener.of(() => { throw new Error('the listener broke'); })];
  const view = new EditorView({ state: EditorState.create({ doc, extensions }), parent: document.body.appendChild(document.createElement('div')) });
  views.push(view);
  let core: EngineCore | null = null;
  const errors: unknown[] = [];
  const host = createCm6Host({
    view: () => view,
    core: () => core,
    languages: () => ['python'],
    language: () => language,
    configure: () => ({ ok: false, error: { code: 'internal', message: 'not under test' } }),
    buildState: (text) => EditorState.create({ doc: text, extensions }),
  });
  core = createEditorEngine(host, { ...options.engine, instance: 'host-test', logError: (error) => errors.push(error) });
  const events: EditorEvent[] = [];
  core.listen(['changed'], (event) => events.push(event));
  return { view, core, events, swallowed, errors };
}

describe('the CodeMirror host', () => {
  it('given an update listener that throws on every update, when edits happen, then the next request still delivers every revision in order', async () => {
    // Given a view whose listener throws, and two edits made with nothing pumping them
    const { view, core, events, swallowed } = mount('abc');
    view.dispatch({ changes: { from: 0, insert: 'x' } });
    view.dispatch({ changes: { from: 0, insert: 'y' } });
    expect(swallowed).toHaveLength(2);
    expect(core.revision()).toBe(0);

    // When a request runs
    const reply = await core.request('read', {});

    // Then both revisions were delivered, in order, each with its own change list
    expect(events).toMatchObject([
      { from: 0, to: 1, changes: [{ from: 0, to: 0, insert: 'x' }], origin: 'api' },
      { from: 1, to: 2, changes: [{ from: 0, to: 0, insert: 'y' }], origin: 'api' },
    ]);
    expect(reply).toMatchObject({ ok: true, revision: 2, value: { text: 'yxabc' } });
  });

  it('given dispatches typed, unannotated and undone, when they are delivered, then they are user, api and history', async () => {
    const { view, core, events } = mount('');
    view.dispatch({ changes: { from: 0, insert: 'a' }, userEvent: 'input.type', selection: { anchor: 1 } });
    view.dispatch({ changes: { from: 1, insert: 'b' } });
    undo(view);
    await core.request('read', {});
    expect(events.map((event) => event.type === 'changed' && event.origin)).toEqual(['user', 'api', 'history']);
    expect(events[0]).toMatchObject({ selection: { ranges: [{ anchor: 1, head: 1 }], main: 0 } });
  });

  it('given more edits than the host keeps between pumps, when they are delivered, then the gap shows in from and a mirror resyncs to the document', async () => {
    // Given a mirror and seventy edits nothing pumped
    const { view, core, events } = mount('');
    const mirror = await createDocumentMirror(core, () => undefined);
    for (let i = 0; i < 70; i += 1) view.dispatch({ changes: { from: view.state.doc.length, insert: String(i % 10) } });

    // When a request runs
    await core.request('read', {});

    // Then the first delivered event does not follow the mirror's revision, and the mirror catches up by reading
    expect(events[0]).toMatchObject({ from: 6, to: 7 });
    await new Promise<void>((resolve, reject) => {
      const deadline = Date.now() + 2000;
      const check = () => (mirror.text() === view.state.doc.toString() ? resolve() : Date.now() > deadline ? reject(new Error('the mirror did not resync')) : setTimeout(check, 10));
      check();
    });
    expect(mirror.revision()).toBe(70);
  });

  const typeStep = (text: string): ApplyParams => ({ steps: [{ op: 'type', text }] });

  it('given the caret at the end, when "(" is typed through the api, then closeBrackets adds the pair as one api change with the caret between, and one undo takes it back', async () => {
    // Given a document and the engine over a view with closeBrackets
    const { view, core, events } = mount('f');
    view.dispatch({ selection: { anchor: 1 } });

    // When a keystroke is typed through the typed API
    const reply = await core.request('apply', typeStep('('));

    // Then the pair is there, the caret sits between, the change is the call's and one undo step
    expect(view.state.doc.toString()).toBe('f()');
    expect(view.state.selection.main.head).toBe(2);
    expect(reply).toMatchObject({ ok: true, value: { landed: [null] } });
    expect(events).toMatchObject([{ origin: 'api', changes: [{ from: 1, to: 1, insert: '()' }] }]);
    undo(view);
    expect(view.state.doc.toString()).toBe('f');
  });

  it('given a pair the api auto-closed, when ")" is typed through the api, then it steps over the closer: the text stays and the caret moves past it', async () => {
    // Given "(" typed through the api, which closed the pair and tracked it
    const { view, core } = mount('f');
    view.dispatch({ selection: { anchor: 1 } });
    await core.request('apply', typeStep('('));

    // When ")" is typed
    const reply = await core.request('apply', typeStep(')'));

    // Then the text is the same, the caret is past the closer, and the one change CodeMirror records replaces the closer with itself
    expect(view.state.doc.toString()).toBe('f()');
    expect(view.state.selection.main.head).toBe(3);
    expect(reply).toMatchObject({ ok: true, value: { changes: [{ from: 2, to: 3, insert: ')' }] } });
  });

  it('given a plain character, when it is typed through the api, then it is inserted at the caret as an api change', async () => {
    const { view, core, events } = mount('ab');
    view.dispatch({ selection: { anchor: 1 } });

    await core.request('apply', typeStep('x'));

    expect(view.state.doc.toString()).toBe('axb');
    expect(events).toMatchObject([{ origin: 'api', changes: [{ from: 1, to: 1, insert: 'x' }] }]);
  });

  it('given the person typing before and after an api keystroke, when undo runs, then there are three steps in that order', async () => {
    const { view, core } = mount('');
    view.dispatch({ changes: { from: 0, insert: 'a' }, userEvent: 'input.type', selection: { anchor: 1 } });
    await core.request('apply', typeStep('b'));
    view.dispatch({ changes: { from: 2, insert: 'c' }, userEvent: 'input.type', selection: { anchor: 3 } });
    expect(view.state.doc.toString()).toBe('abc');

    undo(view);
    expect(view.state.doc.toString()).toBe('ab');
    undo(view);
    expect(view.state.doc.toString()).toBe('a');
    undo(view);
    expect(view.state.doc.toString()).toBe('');
  });

  it('given a view that is not typing through the api, when the person types, then the change is the person\'s: the call marker does not leak', async () => {
    const { view, core, events } = mount('');
    await core.request('apply', typeStep('a'));
    view.dispatch({ changes: { from: 1, insert: 'b' }, userEvent: 'input.type', selection: { anchor: 2 } });
    await core.request('read', {});

    expect(events.map((event) => event.type === 'changed' && event.origin)).toEqual(['api', 'user']);
  });

  it('given a page with the hidden blur button, when the api blurs the editor, then the focus goes to the button and is released next frame', async () => {
    // Given a focused editor in a page that has the button the page html carries
    const { view, core } = mount('abc');
    const button = document.createElement('button');
    button.id = 'kb-blur-target';
    document.body.appendChild(button);
    view.focus();
    expect(view.hasFocus).toBe(true);
    const frames: FrameRequestCallback[] = [];
    const raf = window.requestAnimationFrame;
    window.requestAnimationFrame = (callback) => frames.push(callback);

    try {
      // When the api blurs it
      const reply = await core.request('view', { focus: false });

      // Then the editor has let go with the button holding the focus, and a frame later nothing does
      expect(reply).toMatchObject({ ok: true, value: { focused: false } });
      expect(view.hasFocus).toBe(false);
      expect(document.activeElement).toBe(button);
      frames.forEach((frame) => frame(0));
      expect(document.activeElement).not.toBe(button);
    } finally {
      window.requestAnimationFrame = raf;
      button.remove();
    }
  });

  it('given a page without that button, when the api blurs the editor, then the editor is blurred directly', async () => {
    const { view, core } = mount('abc');
    view.focus();

    const reply = await core.request('view', { focus: false });

    expect(reply).toMatchObject({ ok: true, value: { focused: false } });
    expect(view.hasFocus).toBe(false);
  });
});

describe('the CodeMirror host answering tree', () => {
  const python3 = 'def f(xs):\n    for x in xs:\n        print(x)\n    return 1\n';
  const manyLines = (count: number) => Array.from({ length: count }, (_, index) => `x${index} = ${index} + 1`).join('\n');

  it('given a real EditorView with python(), when the def and for snippet is requested, then the outline has the for inside the def inside the script, with CodeMirror\'s own offsets', async () => {
    const { core } = mount(python3);

    const value = unwrap(await core.request('tree', {}));

    expect(value).toMatchObject({ language: 'python', complete: true, parsedTo: python3.length, root: { type: 'Script', from: 0, to: python3.length } });
    const body = value.root?.children?.[0]?.children?.[0];
    const loop = body?.children?.[0];
    expect(value.root?.children?.[0]?.type).toBe('FunctionDefinition');
    expect(python3.startsWith('for x in xs:', loop?.from)).toBe(true);
    expect(python3.slice(0, loop?.from)).toBe('def f(xs):\n    ');
    expect(enclosingBlock(value, python3, python3.indexOf('print') + 2)?.owner?.type).toBe('ForStatement');
  });

  it('given a c++ view, when a function is requested, then the compound statement is the function body', async () => {
    const source = 'int main() {\n  return 0;\n}\n';
    const { core } = mount(source, { language: 'cpp' });

    const value = unwrap(await core.request('tree', {}));

    const definition = value.root?.children?.[0];
    expect(classify('cpp', definition as never)).toBe('compound');
    expect(definition?.children?.[0]?.type).toBe('CompoundStatement');
  });

  it('given a long document and no budget, when the tree is requested, then it says how far the parse got and that it is not complete; given time, it completes', async () => {
    const doc = manyLines(700);
    const { core } = mount(doc);

    const first = unwrap(await core.request('tree', { budgetMs: 0 }));
    const second = unwrap(await core.request('tree', { budgetMs: 1_000 }));

    expect(doc.length).toBeGreaterThan(6_000);
    expect(first.complete).toBe(false);
    expect(first.parsedTo).toBeLessThan(doc.length);
    expect(second).toMatchObject({ complete: true, parsedTo: doc.length });
  });

  it('given a very long document and a small budget, when the tree is requested, then the engine gave the page turns between slices and a later request with more budget completes', async () => {
    const doc = manyLines(7_000);
    let yields = 0;
    const { core } = mount(doc, { engine: { treeSliceMs: 1, yieldToEvents: async () => { yields += 1; } } });

    const first = unwrap(await core.request('tree', { budgetMs: 15 }));
    const second = unwrap(await core.request('tree', { budgetMs: 1_000 }));

    expect(doc.length).toBeGreaterThan(100_000);
    expect(first.complete).toBe(false);
    expect(yields).toBeGreaterThan(1);
    expect(second.complete).toBe(true);
  });

  it('given an edit while the parse waits for a turn, when the parse resumes, then the reply is incomplete and names the revision before the edit', async () => {
    const doc = manyLines(6_000);
    const release: (() => void)[] = [];
    const { core, view } = mount(doc, { engine: { treeSliceMs: 1, yieldToEvents: () => new Promise<void>((resolve) => release.push(resolve)) } });

    const pending = core.request('tree', { budgetMs: 1_000 });
    await new Promise((resolve) => setTimeout(resolve, 20));
    view.dispatch({ changes: { from: 0, insert: 'y = 0\n' } });
    // a request made while the tree waits delivers the edit to the engine: the queue is not held behind the parse
    await core.request('read', {});
    release.splice(0).forEach((resolve) => resolve());
    const reply = await pending;

    expect(reply).toMatchObject({ ok: true, base: 0, revision: 1, value: { complete: false, revision: 0 } });
  });

  it('given a tree call, then no changed event fires and the revision is unchanged', async () => {
    const { core, events } = mount(python3);

    await core.request('tree', {});
    await core.request('tree', { detail: 'full' });

    expect(events).toEqual([]);
    expect(core.revision()).toBe(0);
  });

  it('given a half-typed for header, when the tree is requested, then it has an error node and the def around it is still found', async () => {
    const broken = 'def f(xs):\n    for x in\n    return 1\n';
    const { core } = mount(broken);

    const value = unwrap(await core.request('tree', {}));

    expect(JSON.stringify(value.root)).toContain('"error":true');
    expect(enclosingBlock(value, broken, broken.indexOf('return') + 3)?.owner?.type).toBe('FunctionDefinition');
  });
});

