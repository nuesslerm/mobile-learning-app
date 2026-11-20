import { isolateHistory, redo, undo } from '@codemirror/commands';
import { Annotation, EditorSelection, StateField, Transaction, type EditorState } from '@codemirror/state';
import type { EditorView } from '@codemirror/view';
import type { Change, ConfigPatch, DocumentHost, EditorConfig, EditorError, EngineCore, Language, Origin, SelRange, Selection } from '@lib/code-editor-api';

type Entry = { revision: number; changes: Change[]; selection: SelRange; origin: Origin; label?: string; length: number };
type RevisionState = { revision: number; tail: readonly Entry[] };

const TAIL = 64;
// marks a transaction the typed API made, and carries its label
export const apiCall = Annotation.define<{ label?: string }>();
let revisionBase = 0;

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
  configure: (patch: ConfigPatch) => { ok: true; config: EditorConfig } | { ok: false; error: EditorError };
  /** a fresh state holding `text` with the page's extensions and the revision field */
  buildState: (text: string, selection?: SelRange) => EditorState;
};

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
      else view().contentDOM.blur();
    },
    languages: deps.languages,
    maxSelections: 1,
  };
  return host;
}
