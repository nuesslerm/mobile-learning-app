import { applyChanges, mapSelRange, normalizeText } from '../core/changes';
import { createEditorEngine, type EngineCore } from '../core/engine';
import type { DocumentHost } from '../ports/host';
import type { Change, ConfigPatch, EditorConfig, Language, Origin, Revision, SelRange, Selection } from '../ports/protocol';
import { emptyHistory, pop, recordChange, recordSelection, type HistoryState } from './history';

export type FakeEditorOptions = {
  readonly text?: string;
  readonly instance?: string;
  readonly logDepth?: number;
  readonly languages?: readonly Language[];
  /** where errors thrown by event listeners and the host go; a test reads them back through `internalErrors` */
  readonly logError?: (error: unknown) => void;
};
export type FakeEditor = {
  readonly engine: EngineCore;
  text(): string;
  /** the person typing: replaces the selection */
  userTypes(text: string): void;
  userSelects(anchor: number, head?: number): void;
  /** moves the injectable clock the undo history groups typing by */
  advance(ms: number): void;
  internalErrors(): readonly unknown[];
  peek(): { text: string; selection: Selection; revision: Revision; undoDepth: number; redoDepth: number; focused: boolean };
};

const DEFAULT_CONFIG: EditorConfig = {
  language: 'python',
  wrap: false,
  tabSize: 4,
  fontSize: 14,
  lineHeight: 20,
  palette: { dark: false, cursor: '#000000', selection: '#cccccc', background: '#ffffff', gutter: '#eeeeee', border: '#dddddd' },
};

const selectionOf = (range: SelRange): Selection => ({ ranges: [range], main: 0 });

/** A string-backed DocumentHost under the same engine core as the real editor: only what the host contributes can differ. */
export function createFakeEditor(options: FakeEditorOptions = {}): FakeEditor {
  let text = normalizeText(options.text ?? '');
  let selection: SelRange = { anchor: 0, head: 0 };
  let config: EditorConfig = DEFAULT_CONFIG;
  let focused = false;
  let clock = 1_000_000;
  let past: HistoryState = emptyHistory;
  const errors: unknown[] = [];
  const sink: { core: EngineCore | null } = { core: null };
  const core = (): EngineCore => {
    if (!sink.core) throw new Error('the fake editor has no engine yet');
    return sink.core;
  };

  const report = (changes: readonly Change[], origin: Origin, label?: string) =>
    core().ingest({ revision: core().revision() + 1, changes, selection: selectionOf(selection), origin, length: text.length, ...(label ? { label } : {}) });

  const edit = (changes: readonly Change[], next: SelRange | undefined, userEvent: string, isolated: boolean, origin: Origin, label?: string) => {
    const before = text;
    const startSelection = selection;
    text = applyChanges(before, changes);
    selection = next ?? mapSelRange(selection, changes);
    past = recordChange(past, { forward: changes, before, after: text, startSelection, time: clock, userEvent, isolated });
    report(changes, origin, label);
  };
  const reselect = (next: SelRange, userEvent: string) => {
    const before = selection;
    selection = next;
    past = recordSelection(past, before, clock, userEvent);
    core().ingestSelection(selectionOf(selection));
  };

  const host: DocumentHost = {
    doc: { get length() { return text.length; }, slice: (from, to) => text.slice(from, to) },
    selection: () => selectionOf(selection),
    hasFocus: () => focused,
    sync: () => undefined,
    commit({ changes, selection: next, label }) {
      if (changes.length === 0) {
        if (next) reselect(next, 'select.api');
        return;
      }
      edit(changes, next, `api.${label ?? 'apply'}`, true, 'api', label);
    },
    history(direction) {
      const popped = pop(past, direction === 'undo' ? 'done' : 'undone', text, selection);
      if (!popped) return false;
      text = applyChanges(text, popped.apply);
      selection = popped.selection;
      past = popped.next;
      report(popped.apply, 'history');
      return true;
    },
    load(next, range) {
      text = normalizeText(next);
      selection = range ?? { anchor: 0, head: 0 };
      past = emptyHistory;
      return { text, selection: selectionOf(selection), revision: core().revision() + 1 };
    },
    configure(patch: ConfigPatch) {
      config = { ...config, ...patch, palette: { ...config.palette, ...patch.palette } };
      return { ok: true, config };
    },
    setFocus(next) {
      focused = next;
      core().ingestFocus(next);
    },
    languages: () => options.languages ?? ['python'],
    maxSelections: 1,
  };

  const engine = createEditorEngine(host, { instance: options.instance ?? 'fake-0', logError: options.logError ?? ((error) => errors.push(error)), ...(options.logDepth === undefined ? {} : { logDepth: options.logDepth }) });
  sink.core = engine;

  return {
    engine,
    text: () => text,
    internalErrors: () => errors,
    userTypes(typed) {
      const insert = normalizeText(typed);
      const from = Math.min(selection.anchor, selection.head);
      const to = Math.max(selection.anchor, selection.head);
      edit([{ from, to, insert }], { anchor: from + insert.length, head: from + insert.length }, 'input.type', false, 'user');
    },
    userSelects(anchor, head = anchor) {
      reselect({ anchor, head }, 'select.pointer');
    },
    advance(ms) {
      clock += ms;
    },
    peek: () => ({ text, selection: selectionOf(selection), revision: core().revision(), undoDepth: past.done.length, redoDepth: past.undone.length, focused }),
  };
}
