import type { EditorEngine } from '../ports/engine';
import type { DocumentHost } from '../ports/host';
import {
  API_VERSION, CORE_REQUEST_NAMES,
  type ApplyValue, type Change, type EditorError, type EditorEvent, type EventType, type Hello, type HistoryValue, type LoadValue, type Mark, type MarkSpec, type Origin, type Range,
  type ReadValue, type Reply, type Requests, type RequestName, type Revision, type Selection, type SelRange, type TreeValue, type ViewValue,
} from '../ports/protocol';
import { lengthDelta, mapSelRange, normalizeText } from './changes';
import { clearMarks, mapMarks, MAX_MARKS, setMarks } from './marks';
import { planBatch, type ResolvedStep } from './plan';
import { createRevisionLog } from './revisionLog';
import { runTreeJob } from './treeJob';
import { isSemanticStep, parseApply, parseConfigPatch, parseHistory, parseLoad, parseRead, parseTree, parseView } from './validate';

export type EngineOptions = {
  readonly instance: string;
  readonly logDepth?: number;
  readonly logError: (error: unknown) => void;
  /** the clock, the turn given to the page's events between parse slices, and a slice's length: injected so a test can pace a parse */
  readonly now?: () => number;
  readonly yieldToEvents?: () => Promise<void>;
  readonly treeSliceMs?: number;
};
export type HostUpdate = {
  readonly revision: Revision;
  readonly changes: readonly Change[];
  readonly selection: Selection;
  readonly origin: Origin;
  readonly label?: string;
  readonly length: number; // the document length after the transaction: how the log restarts after a gap
};
type Deferred = { readonly deferred: Promise<Reply<unknown>> };

export type EngineCore = EditorEngine & {
  readonly instance: string;
  revision(): Revision;
  ingest(update: HostUpdate): void;
  ingestSelection(selection: Selection): void;
  ingestFocus(focused: boolean): void;
  ingestCompletion(open: boolean): void;
};

const DEFAULT_LOG_DEPTH = 256;
const DEFAULT_TREE_BUDGET_MS = 50;
const DEFAULT_TREE_SLICE_MS = 8;
const EXCERPT = 200;

const sameSelection = (a: Selection, b: Selection) =>
  a.main === b.main && a.ranges.length === b.ranges.length && a.ranges.every((range, i) => range.anchor === b.ranges[i]?.anchor && range.head === b.ranges[i]?.head);

export function createEditorEngine(host: DocumentHost, options: EngineOptions): EngineCore {
  const { logError } = options;
  const depth = options.logDepth ?? DEFAULT_LOG_DEPTH;
  const log = createRevisionLog(0, host.doc.length, depth);
  const listeners = new Set<{ readonly types: ReadonlySet<EventType>; readonly fn: (event: EditorEvent) => void }>();
  let lastSelection = host.selection();
  let lastFocus = host.hasFocus();
  let lastCompletion = false;
  let delivered: HostUpdate | null = null;
  let marks: readonly Mark[] = [];
  let revealed: Range | null = null;
  let queue: Promise<unknown> = Promise.resolve();

  const emit = (event: EditorEvent) => {
    for (const listener of [...listeners]) {
      if (!listener.types.has(event.type)) continue;
      try {
        listener.fn(event);
      } catch (error) {
        logError(error);
      }
    }
  };
  const failure = (error: EditorError): Reply<never> => ({ ok: false, revision: log.head(), error });
  const success = <V>(value: V, base: Revision = log.head()): Reply<V> => ({ ok: true, revision: log.head(), base, value });
  const invalid = (path: string, message: string) => failure({ code: 'invalid', message, path });
  const unsupportedView = (what: 'marks' | 'reveal') => failure({ code: 'unsupported', message: `this editor cannot draw ${what}`, what: `view:${what}` });
  const unsupportedStep = (op: string) => failure({ code: 'unsupported', message: `this editor has no step ${op}`, what: `step:${op}` });
  const mainRange = (selection: Selection): Range => {
    const range = selection.ranges[selection.main] ?? { anchor: 0, head: 0 };
    return { from: Math.min(range.anchor, range.head), to: Math.max(range.anchor, range.head) };
  };
  const excerpt = (text: string) => (text.length > EXCERPT ? `${text.slice(0, EXCERPT)}…` : text);

  const hello = (): Reply<Hello> =>
    success({
      api: API_VERSION,
      instance: options.instance,
      ready: true,
      capabilities: {
        // what the host can do is what the engine announces: an optional request, step or event exists only when its method does
        requests: host.tree ? [...CORE_REQUEST_NAMES, 'tree'] : CORE_REQUEST_NAMES,
        steps: ['replace', 'move', 'select', ...(host.type ? (['type'] as const) : []), ...(host.complete ? (['complete'] as const) : [])],
        events: ['ready', 'changed', 'selection', 'focus', ...(host.complete ? (['completion'] as const) : [])],
        languages: host.languages(),
        maxSelections: host.maxSelections,
        logDepth: depth,
        view: [...(host.marks ? (['marks'] as const) : []), ...(host.reveal ? (['reveal'] as const) : [])],
      },
    });

  const read = (params: unknown): Reply<ReadValue> => {
    const parsed = parseRead(params);
    if (!parsed.ok) return failure(parsed.error);
    const length = host.doc.length;
    const range = parsed.value.range ?? { from: 0, to: length };
    if (range.to > length) return failure({ code: 'out-of-range', message: `the range ends past the document (length ${length})`, length });
    return success({ text: host.doc.slice(range.from, range.to), range, length, selection: host.selection() });
  };

  const apply = (params: unknown): Reply<ApplyValue> => {
    const parsed = parseApply(params);
    if (!parsed.ok) return failure(parsed.error);
    const { steps, at, stale = 'reject', instance, label, preview } = parsed.value;
    if (instance !== undefined && instance !== options.instance) {
      return failure({ code: 'instance-mismatch', message: 'these revisions belong to another editor instance', expected: options.instance, actual: instance });
    }
    const head = log.head();
    if (at !== undefined && at > head) return invalid('at', `at ${at} is ahead of the head ${head}`);
    const behind = at !== undefined && at < head;
    if (behind && (stale === 'reject' || at < log.floor())) {
      return failure({ code: 'stale', message: `the call was made at revision ${at}, the head is ${head}`, head, floor: log.floor() });
    }
    if (preview) return failure({ code: 'unsupported', message: 'this editor cannot preview a call', what: 'apply:preview' }); // applying it instead would do the opposite of what was asked
    const sole = steps[0];
    if (sole?.op === 'type' || sole?.op === 'complete') {
      // a keystroke, or the acceptance of a completion, acts on what is selected now, so there is nothing to map: a call made at another revision is stale
      if (sole.op === 'type' ? !host.type : !host.complete) return failure({ code: 'unsupported', message: `this editor has no ${sole.op} step`, what: `step:${sole.op}` });
      if (behind) return failure({ code: 'stale', message: `the call was made at revision ${at}, the head is ${head}`, head, floor: log.floor() });
      delivered = null;
      if (sole.op === 'type') host.type?.(sole.text, label);
      else host.complete?.(label);
      host.sync();
      const update = delivered as HostUpdate | null;
      return success({ changes: update?.changes ?? [], selection: host.selection(), landed: [null] }, head);
    }
    const semantic = steps.find(isSemanticStep);
    if (semantic) return unsupportedStep(semantic.op); // until a planner is registered
    const from = at ?? head;
    const lengthThen = behind ? (log.lengthAt(from) ?? 0) : host.doc.length;
    const live = mainRange(host.selection());
    const outOf = (step: number): EditorError => ({ code: 'out-of-range', message: `step ${step} names text outside revision ${from} (length ${lengthThen})`, length: lengthThen, step });
    const rebasePos = (pos: number, assoc: 1 | -1) => (behind ? log.mapPos(pos, from, assoc) : pos);
    const resolved: ResolvedStep[] = [];
    for (const [index, step] of steps.entries()) {
      if (step.op === 'type' || step.op === 'complete') return invalid(`steps[${index}]`, `a ${step.op} step is the only step of its call`);
      if (isSemanticStep(step)) return unsupportedStep(step.op);
      if (step.op === 'select') {
        const point = (value: typeof step.anchor): typeof step.anchor | EditorError => {
          if (typeof value === 'number') return value > lengthThen ? outOf(index) : rebasePos(value, 1);
          if ('at' in value) return value.at > lengthThen ? outOf(index) : { at: rebasePos(value.at, value.assoc), assoc: value.assoc };
          return value;
        };
        const anchor = point(step.anchor);
        if (typeof anchor === 'object' && 'code' in anchor) return failure(anchor);
        const head = step.head === undefined ? undefined : point(step.head);
        if (typeof head === 'object' && 'code' in head) return failure(head);
        resolved.push(head === undefined ? { op: 'select', anchor } : { op: 'select', anchor, head });
        continue;
      }
      let range: Range;
      if (step.range === 'selection') range = live;
      else if (!behind) range = step.range;
      else {
        if (step.range.to > lengthThen) return failure(outOf(index));
        range = log.mapRange(step.range, from);
        if (step.range.from < step.range.to && range.from === range.to) {
          return failure({ code: 'range-lost', message: `the text step ${index} named is gone from the head`, step: index });
        }
      }
      if (step.expect !== undefined && range.to <= host.doc.length) {
        const actual = host.doc.slice(range.from, range.to);
        if (actual !== step.expect) {
          return failure({ code: 'text-mismatch', message: `step ${index} expected other text`, step: index, expected: excerpt(step.expect), actual: excerpt(actual) });
        }
      }
      if (step.op === 'replace') resolved.push({ op: 'replace', range, text: step.text });
      else {
        if (behind && step.to > lengthThen) return failure(outOf(index));
        resolved.push({ op: 'move', range, to: rebasePos(step.to, 1) });
      }
    }
    const planned = planBatch(host.doc, resolved);
    if (!planned.ok) return failure(planned.error);
    const { plan } = planned;
    if (plan.changes.length === 0 && plan.select === null) {
      return success({ changes: [], selection: host.selection(), landed: plan.landed });
    }
    const main = host.selection().ranges[host.selection().main] ?? { anchor: 0, head: 0 };
    const selection = plan.select ?? (plan.changes.length > 0 ? mapSelRange(main, plan.changes) : null);
    host.commit({ changes: plan.changes, ...(selection ? { selection } : {}), ...(label ? { label } : {}) });
    host.sync();
    if (plan.changes.length > 0 && log.head() === head) {
      return failure({ code: 'internal', message: 'the editor applied the edit and did not report it' });
    }
    return success({ changes: plan.changes, selection: host.selection(), landed: plan.landed }, head);
  };

  const history = (params: unknown): Reply<HistoryValue> => {
    const parsed = parseHistory(params);
    if (!parsed.ok) return failure(parsed.error);
    const base = log.head();
    delivered = null;
    const applied = host.history(parsed.value.direction);
    host.sync();
    const update = delivered as HostUpdate | null;
    return success({ applied, changes: update?.changes ?? [], selection: host.selection() }, base);
  };

  const load = (params: unknown): Reply<LoadValue> => {
    const parsed = parseLoad(params);
    if (!parsed.ok) return failure(parsed.error);
    const text = normalizeText(parsed.value.text);
    const selection: SelRange | undefined = parsed.value.selection;
    if (selection && (selection.anchor > text.length || selection.head > text.length)) {
      return failure({ code: 'out-of-range', message: `the selection is outside the loaded text (length ${text.length})`, length: text.length });
    }
    const base = log.head();
    const oldLength = host.doc.length;
    const loaded = host.load(text, selection);
    log.reset(loaded.revision, loaded.text.length);
    lastSelection = loaded.selection;
    revealed = null;
    if (marks.length > 0) {
      marks = [];
      host.marks?.(marks);
    }
    emit({
      type: 'changed',
      from: loaded.revision - 1,
      to: loaded.revision,
      changes: [{ from: 0, to: oldLength, insert: loaded.text }],
      selection: loaded.selection,
      origin: 'load',
    });
    return success({ length: loaded.text.length, selection: loaded.selection }, base);
  };

  const configure = (params: unknown) => {
    const parsed = parseConfigPatch(params);
    if (!parsed.ok) return failure(parsed.error);
    if (parsed.value.language !== undefined && !host.languages().includes(parsed.value.language)) {
      return failure({ code: 'unsupported', message: `this editor cannot switch to ${parsed.value.language}`, what: 'language' });
    }
    if (parsed.value.completion !== undefined && !host.complete) {
      return failure({ code: 'unsupported', message: 'this editor has no completion', what: 'config:completion' });
    }
    const result = host.configure(parsed.value);
    return result.ok ? success(result.config) : failure(result.error);
  };

  // Everything is checked before anything is applied: a call that fails leaves no mark, no reveal and no focus change behind.
  const view = (params: unknown): Reply<ViewValue> => {
    const parsed = parseView(params);
    if (!parsed.ok) return failure(parsed.error);
    const { focus, at, instance, marks: change, reveal } = parsed.value;
    if (change && !host.marks) return unsupportedView('marks');
    if (reveal && !host.reveal) return unsupportedView('reveal');
    if (instance !== undefined && instance !== options.instance) {
      return failure({ code: 'instance-mismatch', message: 'these revisions belong to another editor instance', expected: options.instance, actual: instance });
    }
    const head = log.head();
    const from = at ?? head;
    if (from > head) return invalid('at', `at ${from} is ahead of the head ${head}`);
    if (from < log.floor()) return failure({ code: 'stale', message: `the call was made at revision ${from}, the head is ${head}`, head, floor: log.floor() });
    const lengthThen = from < head ? (log.lengthAt(from) ?? 0) : host.doc.length;
    const rebase = (range: Range, step: number): Range | EditorError => {
      if (range.to > lengthThen) return { code: 'out-of-range', message: `the range ends past the document at revision ${from} (length ${lengthThen})`, length: lengthThen, step };
      const mapped = from < head ? log.mapRange(range, from) : range;
      return range.from < range.to && mapped.from === mapped.to ? { code: 'range-lost', message: 'the text the range named is gone from the head', step } : mapped;
    };
    const specs: MarkSpec[] = [];
    for (const [index, spec] of (change?.set ?? []).entries()) {
      const range = rebase(spec.range, index);
      if ('code' in range) return failure(range);
      specs.push({ ...spec, range });
    }
    const target = reveal ? rebase(reveal, -1) : null;
    if (target && 'code' in target) return failure(target);
    const next = change ? setMarks(change.clear === undefined ? marks : clearMarks(marks, change.clear), specs) : marks;
    if (next.length > MAX_MARKS) return invalid('marks.set', `a document holds at most ${MAX_MARKS} marks`);

    if (focus !== undefined) host.setFocus(focus);
    if (change) {
      marks = next;
      host.marks?.(marks);
    }
    if (target) {
      revealed = target;
      host.reveal?.(target);
    }
    return success({ focused: host.hasFocus(), marks, revealed });
  };

  // The parse is paced in slices with the page's events given a turn between them, so the call that asked for it is answered
  // later: its stage hands the queue back at once (an apply asked after it is not held behind a parse) and its reply, built after
  // the parse, says which revision the tree describes (`base`) and which the editor had by then (`revision`).
  const now = options.now ?? (() => Date.now());
  const yieldToEvents = options.yieldToEvents ?? (() => new Promise<void>((resolve) => setTimeout(resolve, 0)));
  const sliceMs = options.treeSliceMs ?? DEFAULT_TREE_SLICE_MS;

  const tree = (params: unknown): Reply<TreeValue> | Deferred => {
    const source = host.tree?.();
    if (!source) return failure({ code: 'unsupported', message: 'this editor has no request tree', what: 'request:tree' });
    const parsed = parseTree(params);
    if (!parsed.ok) return failure(parsed.error);
    const length = host.doc.length;
    const range = parsed.value.range ?? { from: 0, to: length };
    if (range.to > length) return failure({ code: 'out-of-range', message: `the range ends past the document (length ${length})`, length });
    const base = log.head();
    const detail = parsed.value.detail ?? 'outline';
    const budgetMs = parsed.value.budgetMs ?? DEFAULT_TREE_BUDGET_MS;
    const deferred = runTreeJob(source, range.to, budgetMs, { now, yieldToEvents, head: () => log.head(), sliceMs })
      .then(
        (result): Reply<unknown> =>
          success(
            { instance: options.instance, revision: base, language: source.language, length: source.length, parsedTo: result.parsedTo, complete: result.complete, detail, root: source.read(range, detail) } satisfies TreeValue,
            base,
          ),
      )
      .catch((error: unknown): Reply<unknown> => {
        logError(error);
        return failure({ code: 'internal', message: error instanceof Error ? error.message : String(error) });
      });
    return { deferred };
  };

  const handlers: Record<RequestName, (params: unknown) => Reply<unknown> | Deferred> = { hello, read, apply, history, load, configure, view, tree };

  const request = (name: string, params: unknown): Promise<Reply<unknown>> => {
    if (name === 'hello') return Promise.resolve(hello());
    const handler = (handlers as Record<string, ((params: unknown) => Reply<unknown> | Deferred) | undefined>)[name];
    if (!handler) return Promise.resolve(failure({ code: 'unsupported', message: `this editor has no request ${name}`, what: `request:${name}` }));
    const stage = queue.then((): Reply<unknown> | Deferred => {
      try {
        host.sync();
        return handler(params);
      } catch (error) {
        logError(error);
        return failure({ code: 'internal', message: error instanceof Error ? error.message : String(error) });
      }
    });
    queue = stage.then(() => undefined);
    return stage.then((result) => ('deferred' in result ? result.deferred : result));
  };

  return {
    instance: options.instance,
    revision: () => log.head(),
    request: request as EditorEngine['request'],
    listen(types, onEvent) {
      const listener = { types: new Set(types), fn: onEvent };
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    ingest(update) {
      if (update.revision <= log.head()) return;
      if (update.revision > log.head() + 1) log.reset(update.revision - 1, update.length - lengthDelta(update.changes));
      log.push(update.changes);
      marks = mapMarks(marks, update.changes);
      revealed = null;
      lastSelection = update.selection;
      delivered = update;
      emit({
        type: 'changed',
        from: update.revision - 1,
        to: update.revision,
        changes: update.changes,
        selection: update.selection,
        origin: update.origin,
        ...(update.label ? { label: update.label } : {}),
      });
    },
    ingestSelection(selection) {
      if (sameSelection(selection, lastSelection)) return;
      lastSelection = selection;
      emit({ type: 'selection', revision: log.head(), selection });
    },
    ingestFocus(focused) {
      if (focused === lastFocus) return;
      lastFocus = focused;
      emit({ type: 'focus', focused });
    },
    ingestCompletion(open) {
      if (open === lastCompletion) return;
      lastCompletion = open;
      emit({ type: 'completion', open, revision: log.head() });
    },
  };
}

/** Answers `hello` (not ready) and `not-ready` until an engine is bound, then delegates; binding again is a new editor instance. */
export function createDeferredEngine(logError: (error: unknown) => void): EditorEngine & { bind(engine: EngineCore): void } {
  let target: EngineCore | null = null;
  const subscriptions = new Set<{ readonly types: readonly EventType[]; readonly fn: (event: EditorEvent) => void; off: (() => void) | null }>();
  const request = (name: string, params: unknown): Promise<Reply<unknown>> => {
    if (target) return target.request(name as RequestName, params as Requests[RequestName]['params']);
    if (name === 'hello') {
      return Promise.resolve({
        ok: true,
        revision: 0,
        base: 0,
        value: { api: API_VERSION, instance: '', ready: false, capabilities: { requests: CORE_REQUEST_NAMES, steps: [], events: [], languages: [], maxSelections: 0, logDepth: 0, view: [] } },
      });
    }
    return Promise.resolve({ ok: false, revision: null, error: { code: 'not-ready', message: 'the editor has not been created yet' } });
  };
  return {
    request: request as EditorEngine['request'],
    listen(types, onEvent) {
      const subscription = { types, fn: onEvent, off: target ? target.listen(types, onEvent) : null };
      subscriptions.add(subscription);
      return () => {
        subscription.off?.();
        subscriptions.delete(subscription);
      };
    },
    bind(engine) {
      for (const subscription of subscriptions) subscription.off?.();
      target = engine;
      for (const subscription of subscriptions) {
        subscription.off = engine.listen(subscription.types, subscription.fn);
        if (!subscription.types.includes('ready')) continue;
        try {
          subscription.fn({ type: 'ready', instance: engine.instance, revision: engine.revision() });
        } catch (error) {
          logError(error);
        }
      }
    },
  };
}
