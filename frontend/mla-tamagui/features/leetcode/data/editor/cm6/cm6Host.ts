import { acceptCompletion } from '@codemirror/autocomplete';
import { isolateHistory, redo, undo } from '@codemirror/commands';
import { ensureSyntaxTree, indentUnit, syntaxTree } from '@codemirror/language';
import { Annotation, EditorSelection, EditorState, StateField, Transaction } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import type { Change, ConfigPatch, DocumentHost, EditorConfig, EditorError, EngineCore, Language, Origin, SelRange, Selection, TreeSnapshot } from '@lib/code-editor-api';

import { setMarksEffect } from './cm6Marks';
import { readSyntaxNode } from './syntaxTreeRead';

type Entry = { revision: number; changes: Change[]; selection: SelRange; origin: Origin; label?: string; length: number };
type RevisionState = { revision: number; tail: readonly Entry[] };

const TAIL = 64;
// marks a transaction the typed API made, and carries its label
export const apiCall = Annotation.define<{ label?: string }>();
let revisionBase = 0;
// set while the host types a keystroke, so the transaction an input handler makes (auto-closed brackets) is the call's too
let typing: { label?: string } | null = null;

/** Marks every document change made while `host.type` runs as the typed API's, in its own undo step. */
export const typingCalls = EditorState.transactionExtender.of((tr) =>
  typing && tr.docChanged ? { annotations: [apiCall.of(typing), isolateHistory.of('full')] } : null
);

/** The revision a state created from now on starts at: the head the typed API had when the document was replaced. */
export function setRevisionBase(revision: number): void {
  revisionBase = revision;
}

const originOf = (tr: Transaction, call: { label?: string } | undefined): Origin => {
  if (tr.isUserEvent('undo') || tr.isUserEvent('redo')) return 'history';
  if (call || tr.isUserEvent('api') || tr.annotation(Transaction.userEvent) === undefined) return 'api';
  return 'user';
};

/**
 * Every document change is one revision, kept with its individual change list. It lives in the state (not a listener-fed counter)
 * because CodeMirror swallows a listener's exception and never calls listeners on `setState`.
 */
export const revisionField = StateField.define<RevisionState>({
  create: () => ({ revision: revisionBase, tail: [] }),
  update(value, tr) {
    if (!tr.docChanged) return value;
    const changes: Change[] = [];
    tr.changes.iterChanges((from, to, _fromB, _toB, text) => changes.push({ from, to, insert: text.toString() }), true);
    const revision = value.revision + 1;
    const call = tr.annotation(apiCall);
    const main = tr.newSelection.main;
    const entry: Entry = {
      revision,
      changes,
      selection: { anchor: main.anchor, head: main.head },
      origin: originOf(tr, call),
      length: tr.newDoc.length,
      ...(call?.label ? { label: call.label } : {}),
    };
    return { revision, tail: [...value.tail, entry].slice(-TAIL) };
  },
});

export const selectionOf = (state: EditorState): Selection => ({
  ranges: state.selection.ranges.map((range) => ({ anchor: range.anchor, head: range.head })),
  main: state.selection.mainIndex,
});

export type Cm6HostDeps = {
  view: () => EditorView;
  /** the engine core the host reports to: set once the engine exists */
  core: () => EngineCore | null;
  languages: () => readonly Language[];
  /** the language the page holds now, which names the grammar's nodes */
  language: () => Language;
  configure: (patch: ConfigPatch) => { ok: true; config: EditorConfig } | { ok: false; error: EditorError };
  /** a fresh state holding `text` with the page's extensions and the revision field */
  buildState: (text: string, selection?: SelRange) => EditorState;
};

/**
 * Lets go of the focus the way the page's html has always done it: iOS WKWebView treats a script `blur()` on the contenteditable as
 * the editor having resigned on purpose and does not bring the keyboard back on the next tap, so the focus goes to a hidden button
 * for a frame instead. A page without that button (a test host) takes the plain blur.
 */
function blurEditor(editor: EditorView): void {
  const doc = editor.contentDOM.ownerDocument;
  const target = doc.getElementById('kb-blur-target');
  if (!target) {
    editor.contentDOM.blur();
    return;
  }
  target.focus();
  (doc.defaultView ?? window).requestAnimationFrame(() => target.blur());
}

/** The syntax tree of `state` as a snapshot: `ensureSyntaxTree` returns the tree it built, which `syntaxTree` of the same state does not show. */
function snapshotOfState(state: EditorState, language: Language): TreeSnapshot {
  let best = syntaxTree(state);
  return {
    language,
    length: state.doc.length,
    work(upto, ms) {
      const done = ensureSyntaxTree(state, upto, ms);
      if (done) best = done;
      return done !== null;
    },
    parsedTo: () => best.length,
    read: (range, detail) => readSyntaxNode(best, language, range, detail),
  };
}

/** The live CodeMirror view as the typed API's document host. */
export function createCm6Host(deps: Cm6HostDeps): DocumentHost {
  const { view } = deps;
  const host: DocumentHost = {
    doc: { get length() { return view().state.doc.length; }, slice: (from, to) => view().state.sliceDoc(from, to) },
    selection: () => selectionOf(view().state),
    hasFocus: () => view().hasFocus,
    // idempotent by revision, so a swallowed listener exception only delays a delivery
    sync() {
      const core = deps.core();
      const state = view().state.field(revisionField, false);
      if (!core || !state) return;
      for (const entry of state.tail) {
        if (entry.revision <= core.revision()) continue;
        core.ingest({
          revision: entry.revision,
          changes: entry.changes,
          selection: { ranges: [entry.selection], main: 0 },
          origin: entry.origin,
          length: entry.length,
          ...(entry.label ? { label: entry.label } : {}),
        });
      }
    },
    type(text, label) {
      const editor = view();
      const { from, to } = editor.state.selection.main;
      // the path a typed key takes: input handlers (closeBrackets) first, then an insert marked as typing, which indentOnInput re-indents
      const insert = () => editor.state.update(editor.state.replaceSelection(text), { scrollIntoView: true, userEvent: 'input.type' });
      typing = label ? { label } : {};
      try {
        const handled = editor.state.facet(EditorView.inputHandler).some((handler) => handler(editor, from, to, text, insert));
        if (!handled) editor.dispatch(insert());
      } finally {
        typing = null;
      }
      host.sync();
      deps.core()?.ingestSelection(selectionOf(editor.state));
    },
    tree: () => snapshotOfState(view().state, deps.language()),
    // a scratch state built the way the page builds its own, so the grammar and its nested config are the live editor's; it is never attached to the view or the revision base
    treeOf: (text) => snapshotOfState(deps.buildState(text), deps.language()),
    indentUnit: () => view().state.facet(indentUnit),
    marks(list) {
      view().dispatch({ effects: setMarksEffect.of(list) });
    },
    reveal({ from, to }) {
      const editor = view();
      const end = editor.state.doc.length;
      editor.dispatch({ effects: EditorView.scrollIntoView(EditorSelection.range(Math.min(from, end), Math.min(to, end)), { y: 'center' }) });
    },
    complete(label) {
      const editor = view();
      // the same marker a typed key gets: the transaction the accepted completion makes is the call's, in its own undo step
      typing = label ? { label } : {};
      let accepted = false;
      try {
        accepted = acceptCompletion(editor);
      } finally {
        typing = null;
      }
      host.sync();
      deps.core()?.ingestSelection(selectionOf(editor.state));
      return accepted;
    },
    commit({ changes, selection, label }) {
      const editor = view();
      editor.dispatch({
        changes: changes.map(({ from, to, insert }) => ({ from, to, insert })),
        ...(selection ? { selection: EditorSelection.single(selection.anchor, selection.head) } : {}),
        userEvent: changes.length ? `api.${label ?? 'apply'}` : 'select.api',
        annotations: changes.length ? [apiCall.of(label ? { label } : {}), isolateHistory.of('full')] : [apiCall.of({})],
      });
      host.sync();
      deps.core()?.ingestSelection(selectionOf(editor.state));
    },
    history(direction) {
      const done = (direction === 'undo' ? undo : redo)(view());
      host.sync();
      return done;
    },
    load(text, selection) {
      host.sync();
      const core = deps.core();
      const revision = (core?.revision() ?? 0) + 1;
      setRevisionBase(revision);
      const editor = view();
      editor.setState(deps.buildState(text, selection));
      return { text: editor.state.doc.toString(), selection: selectionOf(editor.state), revision };
    },
    configure: (patch) => deps.configure(patch),
    setFocus(focused) {
      if (focused) view().focus();
      else blurEditor(view());
    },
    languages: deps.languages,
    maxSelections: 1,
  };
  return host;
}
