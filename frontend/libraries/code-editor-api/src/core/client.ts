import type { EditorEngine } from '../ports/engine';
import type {
  ApplyParams, ApplyValue, ConfigPatch, EditorConfig, EditorError, EditorErrorCode, EditorEvent, EventOf, EventType, Hello, HistoryValue, LoadValue, Point, Pos,
  Range, ReadValue, Reply, RequestName, Requests, Revision, SelRange, Step, ViewValue,
} from '../ports/protocol';

export type CallTrace = {
  readonly name: RequestName;
  readonly ok: boolean;
  readonly code?: EditorErrorCode;
  readonly revision: Revision | null;
  readonly base?: Revision;
  readonly ms?: number;
};
export type ClientOptions = { readonly trace?: (call: CallTrace) => void; readonly now?: () => number };
type Options = Pick<ApplyParams, 'at' | 'stale' | 'instance' | 'label'>;

export type EditorClient = {
  readonly engine: EditorEngine;
  hello(): Promise<Reply<Hello>>;
  getText(range?: Range): Promise<Reply<ReadValue>>;
  transact(steps: readonly Step[], options?: Options): Promise<Reply<ApplyValue>>;
  replaceRange(range: Range | 'selection', text: string, options?: Options): Promise<Reply<ApplyValue>>;
  insertAt(pos: Pos | 'selection', text: string, options?: Options): Promise<Reply<ApplyValue>>;
  moveRange(range: Range | 'selection', to: Pos, options?: Options): Promise<Reply<ApplyValue>>;
  select(anchor: Point, head?: Point, options?: Options): Promise<Reply<ApplyValue>>;
  undo(): Promise<Reply<HistoryValue>>;
  redo(): Promise<Reply<HistoryValue>>;
  load(text: string, selection?: SelRange): Promise<Reply<LoadValue>>;
  setConfig(patch: ConfigPatch): Promise<Reply<EditorConfig>>;
  focus(): Promise<Reply<ViewValue>>;
  blur(): Promise<Reply<ViewValue>>;
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
  return {
    engine,
    hello: () => call('hello', undefined),
    getText: (range) => call('read', range ? { range } : {}),
    transact,
    replaceRange: (range, text, extra) => transact([{ op: 'replace', range, text }], extra),
    insertAt: (pos, text, extra) => transact([{ op: 'replace', range: pos === 'selection' ? 'selection' : { from: pos, to: pos }, text }], extra),
    moveRange: (range, to, extra) => transact([{ op: 'move', range, to }], extra),
    select: (anchor, head, extra) => transact([head === undefined ? { op: 'select', anchor } : { op: 'select', anchor, head }], extra),
    undo: () => call('history', { direction: 'undo' }),
    redo: () => call('history', { direction: 'redo' }),
    load: (text, selection) => call('load', selection ? { text, selection } : { text }),
    setConfig: (patch) => call('configure', patch),
    focus: () => call('view', { focus: true }),
    blur: () => call('view', { focus: false }),
    on: (type, listener) => engine.listen([type], (event: EditorEvent) => listener(event as EventOf<typeof type>)),
  };
}
