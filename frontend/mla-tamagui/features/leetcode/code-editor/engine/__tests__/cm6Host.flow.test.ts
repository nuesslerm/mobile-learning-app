import { closeBrackets } from '@codemirror/autocomplete';
import { history, undo } from '@codemirror/commands';
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { createDocumentMirror, createEditorEngine, type ApplyParams, type EditorEvent, type EngineCore } from '@lib/code-editor-api';
import { afterEach, describe, expect, it } from 'vitest';

import { createCm6Host, revisionField, setRevisionBase, typingCalls } from '../cm6Host';

// A real EditorView in jsdom whose own update listener is broken: the host's pump must not depend on it.
const views: EditorView[] = [];
afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
});

function mount(doc: string) {
  setRevisionBase(0);
  const swallowed: unknown[] = [];
  const extensions = [history(), closeBrackets(), revisionField, typingCalls, EditorView.exceptionSink.of((error) => swallowed.push(error)), EditorView.updateListener.of(() => { throw new Error('the listener broke'); })];
  const view = new EditorView({ state: EditorState.create({ doc, extensions }), parent: document.body.appendChild(document.createElement('div')) });
  views.push(view);
  let core: EngineCore | null = null;
  const errors: unknown[] = [];
  const host = createCm6Host({
    view: () => view,
    core: () => core,
    languages: () => ['python'],
    configure: () => ({ ok: false, error: { code: 'internal', message: 'not under test' } }),
    buildState: (text) => EditorState.create({ doc: text, extensions }),
  });
  core = createEditorEngine(host, { instance: 'host-test', logError: (error) => errors.push(error) });
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
});
