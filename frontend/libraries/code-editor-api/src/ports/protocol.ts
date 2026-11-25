export const API_VERSION = 1 as const;

export type Revision = number; // +1 per document-changing transaction, whoever made it; 0 is the document when the engine was created
export type Pos = number; // UTF-16 code unit offset into the LF-normalised document, as in JS strings and CodeMirror
export type Assoc = -1 | 1; // which side of text inserted at the same position a mapped position stays on
export type Range = { readonly from: Pos; readonly to: Pos };
export type Change = { readonly from: Pos; readonly to: Pos; readonly insert: string }; // positions are in the document the change applies to
export type SelRange = { readonly anchor: Pos; readonly head: Pos };
export type Selection = { readonly ranges: readonly SelRange[]; readonly main: number };

export const LANGUAGE_NAMES = ['python', 'javascript', 'cpp'] as const; // the languages the API can name: an engine offers some of them
export type Language = (typeof LANGUAGE_NAMES)[number];
export type Palette = { readonly dark: boolean; readonly cursor: string; readonly selection: string; readonly background: string; readonly gutter: string; readonly border: string; readonly syntax?: 'default' | 'contrast' };
export type CompletionEntry = { readonly label: string; readonly type?: string; readonly detail?: string; readonly boost?: number }; // a name the editor may offer: a word of a problem's signature
export type CompletionConfig = { readonly enabled: boolean; readonly entries: readonly CompletionEntry[] };
export type EditorConfig = { readonly language: Language; readonly wrap: boolean; readonly whitespace: boolean; readonly tabSize: number; readonly fontSize: number; readonly lineHeight: number; readonly palette: Palette; readonly completion?: CompletionConfig };
export type ConfigPatch = Partial<Omit<EditorConfig, 'palette'>> & { readonly palette?: Partial<Palette> };

export type Target = Range | 'selection'; // an explicit range, or what the engine has selected when the batch runs
// bare Pos and {at,assoc} are base-frame positions mapped through the batch; {step,...} is a result-frame point inside what a step inserted
export type Point =
  | Pos
  | { readonly at: Pos; readonly assoc: Assoc }
  | { readonly step: number; readonly edge?: 'start' | 'end'; readonly offset?: number }; // new: edge defaults to 'start'

export const SEMANTIC_STEP_OPS = ['moveNode', 'wrapNode', 'unwrapNode', 'deleteNode', 'duplicateNode', 'insertSnippet', 'reindentBlock'] as const; // whole-line edits of syntax nodes: an engine announces one only when its host can parse another text
export type SemanticStepOp = (typeof SEMANTIC_STEP_OPS)[number];
// where a node or a snippet goes: beside a reference run of statements, or inside the owner of a block (`block` picks among the blocks of a compound, through its clauses)
export type Destination =
  | { readonly before: Range }
  | { readonly after: Range }
  | { readonly into: Range; readonly at: 'start' | 'end'; readonly block?: number };

export type Step =
  | { readonly op: 'replace'; readonly range: Target; readonly text: string; readonly expect?: string } // insertAt = empty range, delete = text ''
  | { readonly op: 'move'; readonly range: Target; readonly to: Pos; readonly expect?: string }
  | { readonly op: 'select'; readonly anchor: Point; readonly head?: Point; readonly reveal?: boolean } // reveal: scroll the new head into view at the nearest edge, as a typed key does (view.reveal centers); needs view.reveal
  | { readonly op: 'type'; readonly text: string } // one keystroke at the live selection, through the editor's input handlers; the only step of its call
  | { readonly op: 'complete' } // accepts the completion the editor has open, at the live selection; the only step of its call, nothing when none is open
  | { readonly op: 'moveNode'; readonly node: Target; readonly to: Destination; readonly expect?: string }
  | { readonly op: 'wrapNode'; readonly node: Target; readonly header: string; readonly expect?: string } // header: the line without its colon or brace, `for i in range(n)`
  | { readonly op: 'unwrapNode' | 'deleteNode' | 'duplicateNode'; readonly node: Target; readonly expect?: string }
  | { readonly op: 'insertSnippet'; readonly at: Destination; readonly text: string }
  | { readonly op: 'reindentBlock'; readonly node: Target; readonly by?: number }; // by in indent units; omitted = normalise
export type StepOp = Step['op'];
export type SemanticStep = Extract<Step, { readonly op: SemanticStepOp }>;

export type Stale = 'reject' | 'map';
export type ApplyParams = {
  readonly steps: readonly Step[]; // simultaneous, in the coordinates of revision `at`
  readonly at?: Revision; // omitted = the head
  readonly stale?: Stale; // default 'reject'
  readonly instance?: string; // new: the Hello.instance the caller's revisions belong to
  readonly label?: string; // new: /^[a-z0-9][a-z0-9.-]{0,31}$/; becomes userEvent api.<label> and is echoed on the changed event
  readonly preview?: boolean; // plan and check the batch, apply nothing: the reply carries the would-be text
  readonly budgetMs?: number; // 0 to 1000: how long each parse of a semantic step may take (default 250)
};
export type ApplyValue = {
  readonly changes: readonly Change[]; // individual changes in Reply.base coordinates; empty when the text did not change, and on a preview
  readonly selection: Selection;
  readonly landed: readonly (Range | null)[]; // per step, where its inserted text sits in the result; null for select
  readonly preview?: { readonly text: string; readonly diff: readonly Change[]; readonly landed: readonly (Range | null)[] }; // only on a preview: the would-be text, its changes and where each step's text would sit
};
export type ReadParams = { readonly range?: Range };
export type ReadValue = { readonly text: string; readonly range: Range; readonly length: number; readonly selection: Selection };
export type HistoryParams = { readonly direction: 'undo' | 'redo' };
export type HistoryValue = { readonly applied: boolean; readonly changes: readonly Change[]; readonly selection: Selection };
export type LoadParams = { readonly text: string; readonly selection?: SelRange }; // new
export type LoadValue = { readonly length: number; readonly selection: Selection };
export const MARK_KINDS = ['edit', 'error'] as const;
export type MarkKind = (typeof MARK_KINDS)[number];
export type Mark = { readonly id: string; readonly kind: MarkKind; readonly range: Range; readonly hidden?: true }; // a range the editor draws, listed in head coordinates; hidden: listed and kept, not drawn
export type MarkSpec = { readonly id: string; readonly kind: MarkKind; readonly range: Range; readonly hidden?: boolean };
export type ViewParams = {
  readonly focus?: boolean; // all omitted = query
  readonly at?: Revision; // the revision the marks' and the reveal's ranges are in; omitted = the head
  readonly instance?: string; // the Hello.instance those revisions belong to
  readonly marks?: { readonly set?: readonly MarkSpec[]; readonly clear?: readonly string[] | 'all' }; // clear runs before set; at most 8 marks stay, one per id
  readonly reveal?: Range; // scroll the range into view: no focus, no selection, no change to the text
};
export type ViewValue = { readonly focused: boolean; readonly marks: readonly Mark[]; readonly revealed: Range | null }; // revealed: the last reveal, until the text changes

export type TreeDetail = 'outline' | 'full'; // outline: the statements and blocks; full: every named node
export type SyntaxNode = { readonly type: string; readonly from: Pos; readonly to: Pos; readonly error?: true; readonly children?: readonly SyntaxNode[] }; // a Lezer node name and its extent, JSON-safe
export type TreeParams = { readonly range?: Range; readonly detail?: TreeDetail; readonly budgetMs?: number };
export type TreeValue = {
  readonly instance: string; // the tree belongs to (instance, revision): a cached one is keyed by both
  readonly revision: Revision;
  readonly language: Language;
  readonly length: number;
  readonly parsedTo: Pos; // how far the parser got within the budget
  readonly complete: boolean; // parsedTo covers the document (or the asked range)
  readonly detail: TreeDetail;
  readonly root: SyntaxNode | null; // null when nothing of the asked range is parsed yet
};

export type Capabilities = {
  readonly requests: readonly RequestName[];
  readonly steps: readonly StepOp[];
  readonly events: readonly EventType[];
  readonly languages: readonly Language[]; // what configure({language}) accepts now
  readonly maxSelections: number;
  readonly logDepth: number; // how many revisions back `at` still maps to the present
  readonly view: readonly ('marks' | 'reveal')[]; // what `view` can do besides focus
};
export type Hello = { readonly api: typeof API_VERSION; readonly instance: string; readonly ready: boolean; readonly capabilities: Capabilities };

export type Requests = {
  hello: { params: undefined; value: Hello };
  read: { params: ReadParams; value: ReadValue };
  apply: { params: ApplyParams; value: ApplyValue };
  history: { params: HistoryParams; value: HistoryValue };
  load: { params: LoadParams; value: LoadValue };
  configure: { params: ConfigPatch; value: EditorConfig };
  view: { params: ViewParams; value: ViewValue };
  tree: { params: TreeParams; value: TreeValue };
};
export type RequestName = keyof Requests;
export const CORE_REQUEST_NAMES = ['hello', 'read', 'apply', 'history', 'load', 'configure', 'view'] as const satisfies readonly RequestName[]; // every engine answers these
export const OPTIONAL_REQUEST_NAMES = ['tree'] as const satisfies readonly RequestName[]; // announced in hello only by an engine whose host can answer them
export const REQUEST_NAMES = [...CORE_REQUEST_NAMES, ...OPTIONAL_REQUEST_NAMES] as const satisfies readonly RequestName[]; // every name a page serves

export type NotANodeReason = 'no-statement' | 'shares-line' | 'not-in-block' | 'damaged' | 'not-unwrappable';
export type WouldNotParseReason = 'syntax' | 'structure';
export type EditorError =
  | { readonly code: 'not-ready'; readonly message: string }
  | { readonly code: 'invalid'; readonly message: string; readonly path?: string }
  | { readonly code: 'out-of-range'; readonly message: string; readonly length: number; readonly step?: number }
  | { readonly code: 'overlap'; readonly message: string; readonly steps: readonly [number, number] }
  | { readonly code: 'stale'; readonly message: string; readonly head: Revision; readonly floor: Revision }
  | { readonly code: 'instance-mismatch'; readonly message: string; readonly expected: string; readonly actual: string }
  | { readonly code: 'range-lost'; readonly message: string; readonly step: number }
  | { readonly code: 'text-mismatch'; readonly message: string; readonly step: number; readonly expected: string; readonly actual: string }
  | { readonly code: 'unsupported'; readonly message: string; readonly what: string }
  | { readonly code: 'transport'; readonly message: string; readonly cause: 'timeout' | 'closed' | 'protocol' | 'restarted' } // made by the client layer only
  | { readonly code: 'internal'; readonly message: string }
  // reserved for D2/E1/F3: declared now so the union never widens under an exhaustive switch; B1 engines never produce them
  | { readonly code: 'not-a-node'; readonly message: string; readonly range: Range; readonly reason?: NotANodeReason; readonly step?: number }
  | { readonly code: 'would-not-parse'; readonly message: string; readonly errors: readonly Range[]; readonly reason?: WouldNotParseReason } // errors are ranges in the would-be text: nothing applied
  | { readonly code: 'parse-timeout'; readonly message: string; readonly budgetMs: number }
  | { readonly code: 'structure-changed'; readonly message: string; readonly head: Revision }
  | { readonly code: 'conflict'; readonly message: string; readonly step: number; readonly byRevision: Revision };
export type EditorErrorCode = EditorError['code'];

export type Reply<V> = // `revision` is the head after the call, `base` the head it applied to; on a failure, the head with nothing applied (null if no engine answered)
  | { readonly ok: true; readonly revision: Revision; readonly base: Revision; readonly value: V }
  | { readonly ok: false; readonly revision: Revision | null; readonly error: EditorError };

export type Origin = 'user' | 'api' | 'history' | 'load';
export type EditorEvent =
  | { readonly type: 'ready'; readonly instance: string; readonly revision: Revision }
  | { readonly type: 'changed'; readonly from: Revision; readonly to: Revision; readonly changes: readonly Change[]; readonly selection: Selection; readonly origin: Origin; readonly label?: string }
  | { readonly type: 'selection'; readonly revision: Revision; readonly selection: Selection }
  | { readonly type: 'focus'; readonly focused: boolean }
  | { readonly type: 'completion'; readonly open: boolean; readonly revision: Revision } // the editor opened or closed a completion list; once per change
  | { readonly type: 'tree'; readonly revision: Revision; readonly complete: boolean }; // declared now, emitted from D2
export type EventType = EditorEvent['type'];
export type EventOf<T extends EventType> = Extract<EditorEvent, { type: T }>;
