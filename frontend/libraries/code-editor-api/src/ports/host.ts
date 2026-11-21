import type { Change, ConfigPatch, EditorConfig, EditorError, Language, Range, Revision, SelRange, Selection, SyntaxNode, TreeDetail } from './protocol';

export interface DocView { readonly length: number; slice(from: number, to: number): string }
export type Commit = { readonly changes: readonly Change[]; readonly selection?: SelRange; readonly label?: string }; // [] = selection-only; selection is in result coordinates
export type HostLoad = { readonly text: string; readonly selection: Selection; readonly revision: Revision }; // text is the normalised document

/** The syntax tree of the document as it was when the snapshot was taken: parsed in slices the engine paces. */
export interface TreeSnapshot {
  readonly language: Language;
  readonly length: number;
  /** parse towards `upto` for at most `ms` more milliseconds; true when the tree covers `upto` */
  work(upto: number, ms: number): boolean;
  /** how far the parse has reached */
  parsedTo(): number;
  /** the parsed tree as plain nodes, cut to `range` and at `detail`; null when nothing of it is parsed */
  read(range: Range, detail: TreeDetail): SyntaxNode | null;
}

export interface DocumentHost { // the driven port: what the engine core needs from an editor
  readonly doc: DocView;
  selection(): Selection;
  hasFocus(): boolean;
  sync(): void; // deliver to core.ingest every transaction not yet delivered; idempotent, called before each request
  type?(text: string, label?: string): void; // one keystroke at the live selection: input handlers (auto-closed brackets) first, else a replace of the selection; at most one transaction, one undo step; core.ingest has run for it before this returns. Announced as the `type` step only when present
  complete?(label?: string): boolean; // accept the open completion at the live selection: true when one was open; at most one transaction, one undo step; core.ingest has run for it before this returns. Announced as the `complete` step and the `completion` event only when present
  tree?(): TreeSnapshot; // a snapshot of the document's syntax tree; announced as the `tree` request only when present
  commit(edit: Commit): void; // one transaction = one undo step; core.ingest must have run for it before this returns
  history(direction: 'undo' | 'redo'): boolean; // true when something was applied; core.ingest has run
  load(text: string, selection?: SelRange): HostLoad; // replace the document, reset history, next revision = previous head + 1
  configure(patch: ConfigPatch): { readonly ok: true; readonly config: EditorConfig } | { readonly ok: false; readonly error: EditorError };
  setFocus(focused: boolean): void;
  languages(): readonly Language[];
  readonly maxSelections: number;
}
