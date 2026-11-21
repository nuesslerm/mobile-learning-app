import { applyChanges, mapSelRange, normalizeText } from '../core/changes';
import { createEditorEngine, type EngineCore, type EngineOptions } from '../core/engine';
import type { DocumentHost, TreeSnapshot } from '../ports/host';
import { snapshotOf, type TreeParse } from './treeParsers';
import { LANGUAGE_NAMES, type Change, type ConfigPatch, type EditorConfig, type Language, type Origin, type Revision, type SelRange, type Selection } from '../ports/protocol';
import { emptyHistory, pop, recordChange, recordSelection, type HistoryState } from './history';

export type FakeEditorOptions = {
  readonly text?: string;
  readonly instance?: string;
  readonly logDepth?: number;
  readonly languages?: readonly Language[];
  /** false: a host without the optional `type` method, which then does not announce the step */
  readonly typing?: boolean;
  /** false: a host without the optional `complete` method, which then announces neither the step, the event nor the config */
  readonly completion?: boolean;
  /** gives the host an optional `tree()`, which makes the engine announce the request: `true` for the fake's own tiny parsers, `parse` to replace them, or a function that makes the snapshot */
  readonly tree?: true | { readonly parse?: TreeParse } | ((text: string, language: Language) => TreeSnapshot);
  /** the engine's clock, parse turn and slice length, so a test can pace a parse */
  readonly engine?: Pick<EngineOptions, 'now' | 'yieldToEvents' | 'treeSliceMs'>;
  /** where errors thrown by event listeners and the host go; a test reads them back through `internalErrors` */
  readonly logError?: (error: unknown) => void;
};
export type FakeEditor = {
  readonly engine: EngineCore;
  /** the host the engine runs on, for a test of the host itself */
  readonly host: DocumentHost;
  text(): string;
  /** the person typing: replaces the selection */
  userTypes(text: string): void;
  userSelects(anchor: number, head?: number): void;
  /** moves the injectable clock the undo history groups typing by */
  advance(ms: number): void;
  /** the next trees need this many calls to `work` before they cover the text (1: at once) */
  slowParse?(calls: number): void;
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
  let slowBy = 1;
  let completionOpen = false;
  const errors: unknown[] = [];
  const sink: { core: EngineCore | null } = { core: null };
  const core = (): EngineCore => {
    if (!sink.core) throw new Error('the fake editor has no engine yet');
    return sink.core;
  };

  const report = (changes: readonly Change[], origin: Origin, label?: string) =>
    core().ingest({ revision: core().revision() + 1, changes, selection: selectionOf(selection), origin, length: text.length, ...(label ? { label } : {}) });

  // The model is trivial on purpose: the word before the caret, two characters at least, is a strict prefix of an entry. The real editor opens its list later and ranks more.
  const wordBeforeCaret = (): string => (selection.anchor === selection.head ? (/[A-Za-z_]\w*$/.exec(text.slice(0, selection.head))?.[0] ?? '') : '');
  const suggestion = () => {
    const word = wordBeforeCaret();
    if (!config.completion?.enabled || word.length < 2) return null;
    return config.completion.entries.find((entry) => entry.label.length > word.length && entry.label.toLowerCase().startsWith(word.toLowerCase())) ?? null;
  };
  const setCompletion = (open: boolean) => {
    if (open === completionOpen) return;
    completionOpen = open;
    core().ingestCompletion(open);
  };

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
    setCompletion(false);
  };

  const snapshotFor = (source: string, language: Language): TreeSnapshot => {
    const tree = options.tree;
    if (typeof tree === 'function') return tree(source, language);
    return snapshotOf(source, language, typeof tree === 'object' ? tree.parse : undefined, slowBy);
  };

  const host: DocumentHost = {
    doc: { get length() { return text.length; }, slice: (from, to) => text.slice(from, to) },
    selection: () => selectionOf(selection),
    hasFocus: () => focused,
    sync: () => undefined,
    ...(options.typing === false
      ? {}
      : {
          type(typed: string, label?: string) {
            const insert = normalizeText(typed);
            const from = Math.min(selection.anchor, selection.head);
            const to = Math.max(selection.anchor, selection.head);
            edit([{ from, to, insert }], { anchor: from + insert.length, head: from + insert.length }, `api.${label ?? 'type'}`, true, 'api', label);
            setCompletion(suggestion() !== null);
          },
        }),
    ...(options.completion === false
      ? {}
      : {
          complete(label?: string) {
            const hit = completionOpen ? suggestion() : null;
            if (!hit) return false;
            const to = selection.head;
            const from = to - wordBeforeCaret().length;
            edit([{ from, to, insert: hit.label }], { anchor: from + hit.label.length, head: from + hit.label.length }, `api.${label ?? 'complete'}`, true, 'api', label);
            setCompletion(false);
            return true;
          },
        }),
    ...(options.tree ? { tree: (): TreeSnapshot => snapshotFor(text, config.language), treeOf: (other: string): TreeSnapshot => snapshotFor(other, config.language) } : {}),
    indentUnit: () => ' '.repeat(config.tabSize),
    commit({ changes, selection: next, label }) {
      if (changes.length === 0) {
        if (next) reselect(next, 'select.api');
        return;
      }
      edit(changes, next, `api.${label ?? 'apply'}`, true, 'api', label);
      setCompletion(false);
    },
    history(direction) {
      const popped = pop(past, direction === 'undo' ? 'done' : 'undone', text, selection);
      if (!popped) return false;
      text = applyChanges(text, popped.apply);
      selection = popped.selection;
      past = popped.next;
      report(popped.apply, 'history');
      setCompletion(false);
      return true;
    },
    load(next, range) {
      text = normalizeText(next);
      selection = range ?? { anchor: 0, head: 0 };
      past = emptyHistory;
      setCompletion(false);
      return { text, selection: selectionOf(selection), revision: core().revision() + 1 };
    },
    configure(patch: ConfigPatch) {
      config = { ...config, ...patch, palette: { ...config.palette, ...patch.palette } };
      if (patch.completion) setCompletion(false);
      return { ok: true, config };
    },
    setFocus(next) {
      focused = next;
      core().ingestFocus(next);
    },
    languages: () => options.languages ?? LANGUAGE_NAMES,
    maxSelections: 1,
  };

  const engine = createEditorEngine(host, { ...options.engine, instance: options.instance ?? 'fake-0', logError: options.logError ?? ((error) => errors.push(error)), ...(options.logDepth === undefined ? {} : { logDepth: options.logDepth }) });
  sink.core = engine;

  return {
    engine,
    host,
    text: () => text,
    internalErrors: () => errors,
    userTypes(typed) {
      const insert = normalizeText(typed);
      const from = Math.min(selection.anchor, selection.head);
      const to = Math.max(selection.anchor, selection.head);
      edit([{ from, to, insert }], { anchor: from + insert.length, head: from + insert.length }, 'input.type', false, 'user');
      setCompletion(suggestion() !== null);
    },
    userSelects(anchor, head = anchor) {
      reselect({ anchor, head }, 'select.pointer');
    },
    advance(ms) {
      clock += ms;
    },
    slowParse(calls) {
      slowBy = calls;
    },
    peek: () => ({ text, selection: selectionOf(selection), revision: core().revision(), undoDepth: past.done.length, redoDepth: past.undone.length, focused }),
  };
}
