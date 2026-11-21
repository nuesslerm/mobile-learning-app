import { history, isolateHistory, redo, redoDepth, undo, undoDepth } from '@codemirror/commands';
import { Annotation, EditorSelection, EditorState, Transaction, type SelectionRange } from '@codemirror/state';

import { createEditorEngine, type EngineCore } from '../../src/core/engine';
import type { DocumentHost } from '../../src/ports/host';
import type { Change, ConfigPatch, EditorConfig, Origin, SelRange, Selection } from '../../src/ports/protocol';
import type { FakeEditor } from '../../src/testing';

const apiCall = Annotation.define<{ label?: string }>();

const rangeOf = (range: SelectionRange): SelRange => ({ anchor: range.anchor, head: range.head });
const selectionOf = (state: EditorState): Selection => ({ ranges: state.selection.ranges.map(rangeOf), main: state.selection.mainIndex });

/** The real CodeMirror state and history (no DOM) under the same engine core as the fake: what the differential test compares the fake with. */
export function createHeadlessCm(options: { text?: string; instance?: string; logDepth?: number } = {}): FakeEditor & { rawState(): EditorState } {
  const extensions = [history({ newGroupDelay: 500 })];
  let state = EditorState.create({ doc: options.text ?? '', extensions });
  let clock = 1_000_000;
  let focused = false;
  let config: EditorConfig = {
    language: 'python', wrap: false, tabSize: 4, fontSize: 14, lineHeight: 20,
    palette: { dark: false, cursor: '#000000', selection: '#cccccc', background: '#ffffff', gutter: '#eeeeee', border: '#dddddd' },
  };
  const errors: unknown[] = [];
  const sink: { core: EngineCore | null } = { core: null };
  const core = (): EngineCore => {
    if (!sink.core) throw new Error('no engine yet');
    return sink.core;
  };

  const originOf = (tr: Transaction): Origin => {
    if (tr.isUserEvent('undo') || tr.isUserEvent('redo')) return 'history';
    return tr.annotation(apiCall) ? 'api' : 'user';
  };
  const dispatch = (tr: Transaction) => {
    state = tr.state;
    if (tr.docChanged) {
      const changes: Change[] = [];
      tr.changes.iterChanges((from, to, _fromB, _toB, text) => changes.push({ from, to, insert: text.toString() }), true);
      const label = tr.annotation(apiCall)?.label;
      core().ingest({ revision: core().revision() + 1, changes, selection: selectionOf(state), origin: originOf(tr), length: state.doc.length, ...(label ? { label } : {}) });
    } else if (tr.selection) core().ingestSelection(selectionOf(state));
  };

  const host: DocumentHost = {
    doc: { get length() { return state.doc.length; }, slice: (from, to) => state.sliceDoc(from, to) },
    selection: () => selectionOf(state),
    hasFocus: () => focused,
    sync: () => undefined,
    commit({ changes, selection, label }) {
      dispatch(
        state.update({
          changes: changes.map(({ from, to, insert }) => ({ from, to, insert })),
          ...(selection ? { selection: EditorSelection.single(selection.anchor, selection.head) } : {}),
          userEvent: changes.length ? `api.${label ?? 'apply'}` : 'select.api',
          annotations: changes.length ? [apiCall.of(label ? { label } : {}), isolateHistory.of('full'), Transaction.time.of(clock)] : [apiCall.of({}), Transaction.time.of(clock)],
        }),
      );
    },
    history: (direction) => (direction === 'undo' ? undo : redo)({ state, dispatch }),
    load(text, selection) {
      state = EditorState.create({ doc: text, ...(selection ? { selection: EditorSelection.single(selection.anchor, selection.head) } : {}), extensions });
      return { text: state.doc.toString(), selection: selectionOf(state), revision: core().revision() + 1 };
    },
    configure(patch: ConfigPatch) {
      config = { ...config, ...patch, palette: { ...config.palette, ...patch.palette } };
      return { ok: true, config };
    },
    setFocus(next) {
      focused = next;
      core().ingestFocus(next);
    },
    languages: () => ['python'],
    maxSelections: 1,
  };

  const engine = createEditorEngine(host, { instance: options.instance ?? 'cm-0', logError: (error) => errors.push(error), ...(options.logDepth === undefined ? {} : { logDepth: options.logDepth }) });
  sink.core = engine;
  return {
    engine,
    host,
    text: () => state.doc.toString(),
    userTypes: (text) => dispatch(state.update(state.replaceSelection(text), { userEvent: 'input.type', annotations: Transaction.time.of(clock) })),
    userSelects: (anchor, head = anchor) => dispatch(state.update({ selection: EditorSelection.single(anchor, head), userEvent: 'select.pointer', annotations: Transaction.time.of(clock) })),
    advance: (ms) => {
      clock += ms;
    },
    rawState: () => state,
    internalErrors: () => errors,
    peek: () => ({ text: state.doc.toString(), selection: selectionOf(state), revision: engine.revision(), undoDepth: undoDepth(state), redoDepth: redoDepth(state), focused, marks: [], revealed: null }),
  };
}
