import type { EditorEngine } from '../ports/engine';
import type {
  ApplyParams, ApplyValue, ConfigPatch, Destination, EditorConfig, EditorError, EditorErrorCode, EditorEvent, EventOf, EventType, Hello, HistoryValue, LoadValue, Point, Pos,
  MarkSpec, Range, ReadValue, Reply, RequestName, Requests, Revision, SelRange, Step, SyntaxNode, TreeParams, TreeValue, ViewParams, ViewValue,
} from '../ports/protocol';
import { enclosingBlock, isUnread, nodeAt, nodesInRange, statementAt, type EnclosingBlock, type Side } from './syntax';

export type CallTrace = {
  readonly name: RequestName;
  readonly ok: boolean;
  readonly code?: EditorErrorCode;
  readonly revision: Revision | null;
  readonly base?: Revision;
  readonly ms?: number;
};
export type ClientOptions = { readonly trace?: (call: CallTrace) => void; readonly now?: () => number };
type Options = Pick<ApplyParams, 'at' | 'stale' | 'instance' | 'label' | 'preview' | 'budgetMs'>;
type ViewOptions = Pick<ViewParams, 'at' | 'instance'>;

export type EditorClient = {
  readonly engine: EditorEngine;
  hello(): Promise<Reply<Hello>>;
  getText(range?: Range): Promise<Reply<ReadValue>>;
  transact(steps: readonly Step[], options?: Options): Promise<Reply<ApplyValue>>;
  replaceRange(range: Range | 'selection', text: string, options?: Options): Promise<Reply<ApplyValue>>;
  insertAt(pos: Pos | 'selection', text: string, options?: Options): Promise<Reply<ApplyValue>>;
  moveRange(range: Range | 'selection', to: Pos, options?: Options): Promise<Reply<ApplyValue>>;
  /** moves the whole lines of the statement or run of statements `node` names, the comment lines above it with them, to a destination, at the indent the destination's block has; refused as `invalid` when the destination is inside the node */
  moveNode(node: Range | 'selection', to: Destination, options?: Options & { readonly expect?: string }): Promise<Reply<ApplyValue>>;
  /** copies the whole lines of the statement or run of statements `node` names right after it; `landed` names the copy */
  duplicateNode(node: Range | 'selection', options?: Options & { readonly expect?: string }): Promise<Reply<ApplyValue>>;
  /** puts the whole lines of the run under a new compound statement, one indent unit deeper; `header` is its line without the colon or brace (`for i in range(n)`, `for (const x of xs)`), and it must make a block that holds exactly the run */
  wrapNode(node: Range | 'selection', header: string, options?: Options & { readonly expect?: string }): Promise<Reply<ApplyValue>>;
  /** replaces a compound statement that has one block with the whole lines of that block, one indent unit shallower; refused as `not-a-node` (`not-unwrappable`) for an `if` with an `else`, a `try` or a decorated function */
  unwrapNode(node: Range | 'selection', options?: Options & { readonly expect?: string }): Promise<Reply<ApplyValue>>;
  /** removes the whole lines of the statement or run of statements `node` names (an empty 'selection' is the statement at the caret), with the comment lines above it; refused as `would-not-parse` when the code would break */
  deleteNode(node: Range | 'selection', options?: Options & { readonly expect?: string }): Promise<Reply<ApplyValue>>;
  select(anchor: Point, head?: Point, options?: Options): Promise<Reply<ApplyValue>>;
  undo(): Promise<Reply<HistoryValue>>;
  redo(): Promise<Reply<HistoryValue>>;
  load(text: string, selection?: SelRange): Promise<Reply<LoadValue>>;
  setConfig(patch: ConfigPatch): Promise<Reply<EditorConfig>>;
  focus(): Promise<Reply<ViewValue>>;
  blur(): Promise<Reply<ViewValue>>;
  /** the marks the engine holds and the last range it revealed, only when the engine announces `view.marks` (setting one is `unsupported` otherwise) */
  marks(): Promise<Reply<ViewValue>>;
  /** one atomic call: `clear` runs first, then the specs are set (a spec with the id of a mark replaces it) and `reveal` scrolls to a range; `at` names the revision the ranges are in */
  setMarks(specs: readonly MarkSpec[], options?: ViewOptions & { readonly reveal?: Range; readonly clear?: readonly string[] | 'all' }): Promise<Reply<ViewValue>>;
  clearMarks(ids: readonly string[] | 'all', options?: ViewOptions): Promise<Reply<ViewValue>>;
  reveal(range: Range, options?: ViewOptions): Promise<Reply<ViewValue>>;
  /** the syntax tree, only when the engine announces `tree` (else `unsupported`); `complete: false` says the parse stopped at its budget */
  tree(params?: TreeParams): Promise<Reply<TreeValue>>;
  /** the node at `pos` (full detail); null where the tree is not read. The reply's `base` is the revision the tree describes. */
  nodeAt(pos: Pos, options?: { readonly side?: Side }): Promise<Reply<SyntaxNode | null>>;
  nodesInRange(range: Range, options?: { readonly within?: boolean }): Promise<Reply<readonly SyntaxNode[]>>;
  statementAt(pos: Pos): Promise<Reply<SyntaxNode | null>>;
  /** the block a caret at `pos` types into; reads the text too, unless `text` (a mirror's) is given, and a keystroke between the two reads is retried once, then `stale` */
  enclosingBlock(pos: Pos, options?: { readonly text?: string }): Promise<Reply<EnclosingBlock | null>>;
  on<T extends EventType>(type: T, listener: (event: EventOf<T>) => void): () => void;
};

export class EditorCallError extends Error {
  readonly error: EditorError;
  constructor(error: EditorError) {
    super(`${error.code}: ${error.message}`);
    this.name = 'EditorCallError';
    this.error = error;
  }
}

/** For tests and scripts, never for UI: a failed reply becomes a throw. */
export function unwrap<V>(reply: Reply<V>): V {
  if (!reply.ok) throw new EditorCallError(reply.error);
  return reply.value;
}

const CAUSES = ['timeout', 'closed', 'protocol', 'restarted'] as const;
const causeOf = (error: unknown): (typeof CAUSES)[number] => {
  const code = typeof error === 'object' && error !== null && 'code' in error ? (error as { code: unknown }).code : undefined;
  return CAUSES.find((cause) => cause === code) ?? 'protocol';
};

/** Where the text the tree must cover starts: the nearest line above the caret's line that has anything on it, so a blank line still finds the blocks that ended just above. */
function previousContentLine(text: string, pos: Pos): Pos {
  const startOf = (at: Pos): Pos => (at <= 0 ? 0 : text.lastIndexOf('\n', at - 1) + 1);
  let from = startOf(pos);
  while (from > 0) {
    const above = startOf(from - 1);
    const blank = text.slice(above, from - 1).trim() === '';
    from = above;
    if (!blank) break;
  }
  return from;
}

/** Named calls are sugar over `engine.request` and never reject: a transport failure resolves as {ok: false, error: {code: 'transport'}}. */
export function createEditorClient(engine: EditorEngine, options: ClientOptions = {}): EditorClient {
  const now = options.now ?? (() => Date.now());
  const call = async <K extends RequestName>(name: K, params: Requests[K]['params']): Promise<Reply<Requests[K]['value']>> => {
    const started = now();
    const reply = await engine.request(name, params).then(
      (answer): Reply<Requests[K]['value']> => answer,
      (error: unknown): Reply<Requests[K]['value']> => ({ ok: false, revision: null, error: { code: 'transport', message: error instanceof Error ? error.message : String(error), cause: causeOf(error) } }),
    );
    options.trace?.({
      name,
      ok: reply.ok,
      revision: reply.revision,
      ms: now() - started,
      ...(reply.ok ? { base: reply.base } : { code: reply.error.code }),
    });
    return reply;
  };
  const transact = (steps: readonly Step[], extra: Options = {}) => call('apply', { steps, ...extra });
  const through = <T>(reply: Reply<TreeValue>, map: (value: TreeValue) => T): Reply<T> => (reply.ok ? { ok: true, revision: reply.revision, base: reply.base, value: map(reply.value) } : reply);
  const pointAt = (pos: Pos): Range => ({ from: pos, to: pos });

  const blockAt = async (pos: Pos, given: string | undefined): Promise<Reply<EnclosingBlock | null>> => {
    let last: Reply<EnclosingBlock | null> | null = null;
    for (let attempt = 0; attempt < 2; attempt += 1) {
      const read = given === undefined ? await call('read', {}) : null;
      if (read && !read.ok) return read;
      const text = read && read.ok ? read.value.text : (given as string);
      const answer = await call('tree', { range: { from: previousContentLine(text, pos), to: pos }, detail: 'outline' });
      if (!answer.ok) return answer;
      if (!read || (read.ok && read.revision === answer.base)) return through(answer, (value) => enclosingBlock(value, text, pos));
      last = { ok: false, revision: answer.revision, error: { code: 'stale', message: `the text was read at revision ${read.revision as number} and the tree describes ${answer.base}`, head: answer.revision, floor: Math.min(answer.base, read.revision as number) } };
    }
    return last as Reply<EnclosingBlock | null>;
  };

  return {
    engine,
    hello: () => call('hello', undefined),
    getText: (range) => call('read', range ? { range } : {}),
    transact,
    replaceRange: (range, text, extra) => transact([{ op: 'replace', range, text }], extra),
    insertAt: (pos, text, extra) => transact([{ op: 'replace', range: pos === 'selection' ? 'selection' : { from: pos, to: pos }, text }], extra),
    moveRange: (range, to, extra) => transact([{ op: 'move', range, to }], extra),
    moveNode: (node, to, { expect, ...extra } = {}) => transact([{ op: 'moveNode', node, to, ...(expect === undefined ? {} : { expect }) }], extra),
    duplicateNode: (node, { expect, ...extra } = {}) => transact([{ op: 'duplicateNode', node, ...(expect === undefined ? {} : { expect }) }], extra),
    wrapNode: (node, header, { expect, ...extra } = {}) => transact([{ op: 'wrapNode', node, header, ...(expect === undefined ? {} : { expect }) }], extra),
    unwrapNode: (node, { expect, ...extra } = {}) => transact([{ op: 'unwrapNode', node, ...(expect === undefined ? {} : { expect }) }], extra),
    deleteNode: (node, { expect, ...extra } = {}) => transact([{ op: 'deleteNode', node, ...(expect === undefined ? {} : { expect }) }], extra),
    select: (anchor, head, extra) => transact([head === undefined ? { op: 'select', anchor } : { op: 'select', anchor, head }], extra),
    undo: () => call('history', { direction: 'undo' }),
    redo: () => call('history', { direction: 'redo' }),
    load: (text, selection) => call('load', selection ? { text, selection } : { text }),
    setConfig: (patch) => call('configure', patch),
    focus: () => call('view', { focus: true }),
    blur: () => call('view', { focus: false }),
    marks: () => call('view', {}),
    setMarks: (specs, { reveal, clear, ...rest } = {}) => call('view', { ...rest, marks: { set: specs, ...(clear === undefined ? {} : { clear }) }, ...(reveal ? { reveal } : {}) }),
    clearMarks: (ids, options = {}) => call('view', { ...options, marks: { clear: ids } }),
    reveal: (range, options = {}) => call('view', { ...options, reveal: range }),
    tree: (params = {}) => call('tree', params),
    nodeAt: async (pos, options = {}) => through(await call('tree', { range: pointAt(pos), detail: 'full' }), (value) => (isUnread(value, pos) ? null : nodeAt(value.root, pos, options.side))),
    nodesInRange: async (range, options = {}) => through(await call('tree', { range, detail: 'full' }), (value) => nodesInRange(value.root, range, options)),
    statementAt: async (pos) => through(await call('tree', { range: pointAt(pos), detail: 'outline' }), (value) => statementAt(value, pos)),
    enclosingBlock: (pos, options = {}) => blockAt(pos, options.text),
    on: (type, listener) => engine.listen([type], (event: EditorEvent) => listener(event as EventOf<typeof type>)),
  };
}
