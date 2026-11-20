export const API_VERSION = 1 as const;

export type Revision = number; // +1 per document-changing transaction, whoever made it; 0 is the document when the engine was created
export type Pos = number; // UTF-16 code unit offset into the LF-normalised document, as in JS strings and CodeMirror
export type Assoc = -1 | 1; // which side of text inserted at the same position a mapped position stays on
export type Range = { readonly from: Pos; readonly to: Pos };
export type Change = { readonly from: Pos; readonly to: Pos; readonly insert: string }; // positions are in the document the change applies to
export type SelRange = { readonly anchor: Pos; readonly head: Pos };
export type Selection = { readonly ranges: readonly SelRange[]; readonly main: number };

export type Language = 'python' | 'javascript' | 'cpp';
export type Palette = { readonly dark: boolean; readonly cursor: string; readonly selection: string; readonly background: string; readonly gutter: string; readonly border: string };
export type EditorConfig = { readonly language: Language; readonly wrap: boolean; readonly tabSize: number; readonly fontSize: number; readonly lineHeight: number; readonly palette: Palette };
export type ConfigPatch = Partial<Omit<EditorConfig, 'palette'>> & { readonly palette?: Partial<Palette> };

export type Target = Range | 'selection'; // an explicit range, or what the engine has selected when the batch runs
// bare Pos and {at,assoc} are base-frame positions mapped through the batch; {step,...} is a result-frame point inside what a step inserted
export type Point =
  | Pos
  | { readonly at: Pos; readonly assoc: Assoc }
  | { readonly step: number; readonly edge?: 'start' | 'end'; readonly offset?: number }; // new: edge defaults to 'start'

export type Step =
  | { readonly op: 'replace'; readonly range: Target; readonly text: string; readonly expect?: string } // insertAt = empty range, delete = text ''
  | { readonly op: 'move'; readonly range: Target; readonly to: Pos; readonly expect?: string }
  | { readonly op: 'select'; readonly anchor: Point; readonly head?: Point }
  | { readonly op: 'type'; readonly text: string }; // one keystroke at the live selection, through the editor's input handlers; the only step of its call
export type StepOp = Step['op'];

export type Stale = 'reject' | 'map';
export type ApplyParams = {
  readonly steps: readonly Step[]; // simultaneous, in the coordinates of revision `at`
  readonly at?: Revision; // omitted = the head
  readonly stale?: Stale; // default 'reject'
  readonly instance?: string; // new: the Hello.instance the caller's revisions belong to
  readonly label?: string; // new: /^[a-z0-9][a-z0-9.-]{0,31}$/; becomes userEvent api.<label> and is echoed on the changed event
};
export type ApplyValue = {
  readonly changes: readonly Change[]; // individual changes in Reply.base coordinates; empty when the text did not change
  readonly selection: Selection;
  readonly landed: readonly (Range | null)[]; // per step, where its inserted text sits in the result; null for select
};
export type ReadParams = { readonly range?: Range };
export type ReadValue = { readonly text: string; readonly range: Range; readonly length: number; readonly selection: Selection };
export type HistoryParams = { readonly direction: 'undo' | 'redo' };
export type HistoryValue = { readonly applied: boolean; readonly changes: readonly Change[]; readonly selection: Selection };
export type LoadParams = { readonly text: string; readonly selection?: SelRange }; // new
export type LoadValue = { readonly length: number; readonly selection: Selection };
export type ViewParams = { readonly focus?: boolean }; // omitted = query
export type ViewValue = { readonly focused: boolean };

export type Capabilities = {
  readonly requests: readonly RequestName[];
  readonly steps: readonly StepOp[];
  readonly events: readonly EventType[];
  readonly languages: readonly Language[]; // what configure({language}) accepts now
  readonly maxSelections: number;
  readonly logDepth: number; // how many revisions back `at` still maps to the present
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
};
export type RequestName = keyof Requests;
export const REQUEST_NAMES = ['hello', 'read', 'apply', 'history', 'load', 'configure', 'view'] as const satisfies readonly RequestName[];

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
  | { readonly code: 'not-a-node'; readonly message: string; readonly range: Range }
  | { readonly code: 'would-not-parse'; readonly message: string; readonly errors: readonly Range[] }
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
  | { readonly type: 'tree'; readonly revision: Revision; readonly complete: boolean }; // declared now, emitted from D2
export type EventType = EditorEvent['type'];
export type EventOf<T extends EventType> = Extract<EditorEvent, { type: T }>;
