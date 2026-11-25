import {
  LANGUAGE_NAMES, MARK_KINDS, SEMANTIC_STEP_OPS,
  type CompletionConfig, type CompletionEntry, type TreeParams,
  type ApplyParams, type ConfigPatch, type Destination, type EditorError, type HistoryParams, type Language, type LoadParams, type MarkKind, type MarkSpec, type Palette, type Point, type Range, type ReadParams, type SelRange, type SemanticStep, type SemanticStepOp, type Step, type ViewParams,
} from '../ports/protocol';
import { normalizeText } from './changes';
import { MAX_MARKS } from './marks';

export type Parsed<T> = { readonly ok: true; readonly value: T } | { readonly ok: false; readonly error: EditorError };

const MAX_STEPS = 10_000;
const MAX_TYPED = 64; // a keystroke, not a paste
const MAX_COMPLETIONS = 200;
const COMPLETION_LABEL = /^[\w$]{1,64}$/;
const MAX_TREE_BUDGET_MS = 1000;
const MAX_HEADER = 200;
const MAX_SNIPPET = 4000;
const MAX_REINDENT = 8;
const LABEL = /^[a-z0-9][a-z0-9.-]{0,31}$/;
const PALETTE_KEYS = ['cursor', 'selection', 'background', 'gutter', 'border'] as const;

const bad = (path: string, message: string): Parsed<never> => ({ ok: false, error: { code: 'invalid', message, path } });
const isObject = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);
const isInt = (value: unknown): value is number => typeof value === 'number' && Number.isInteger(value) && value >= 0;

function parseRange(value: unknown, path: string): Parsed<Range> {
  if (!isObject(value) || !isInt(value.from) || !isInt(value.to)) return bad(path, 'a range is {from, to} of non-negative integers');
  if (value.from > value.to) return bad(path, 'a range has from <= to');
  return { ok: true, value: { from: value.from, to: value.to } };
}

function parsePoint(value: unknown, path: string, steps: readonly unknown[]): Parsed<Point> {
  if (isInt(value)) return { ok: true, value };
  if (!isObject(value)) return bad(path, 'a point is a position, {at, assoc} or {step}');
  if ('at' in value) {
    if (!isInt(value.at) || (value.assoc !== 1 && value.assoc !== -1)) return bad(path, '{at, assoc} needs a non-negative integer and an assoc of -1 or 1');
    return { ok: true, value: { at: value.at, assoc: value.assoc } };
  }
  if (!isInt(value.step) || value.step >= steps.length) return bad(`${path}.step`, 'a point names an existing step');
  const target = steps[value.step];
  if (isObject(target) && target.op === 'select') return bad(`${path}.step`, 'a point cannot name a select step');
  if (value.edge !== undefined && value.edge !== 'start' && value.edge !== 'end') return bad(`${path}.edge`, 'edge is start or end');
  if (value.offset !== undefined && !isInt(value.offset)) return bad(`${path}.offset`, 'offset is a non-negative integer');
  return { ok: true, value: { step: value.step, ...(value.edge ? { edge: value.edge } : {}), ...(value.offset === undefined ? {} : { offset: value.offset }) } };
}

function parseTarget(value: unknown, path: string): Parsed<Range | 'selection'> {
  return value === 'selection' ? { ok: true, value } : parseRange(value, path);
}

function parseDestination(value: unknown, path: string): Parsed<Destination> {
  if (!isObject(value)) return bad(path, 'a destination is {before}, {after} or {into, at}');
  const named = (['before', 'after', 'into'] as const).filter((key) => value[key] !== undefined);
  if (named.length !== 1) return bad(path, 'a destination names exactly one of before, after or into');
  const key = named[0] as 'before' | 'after' | 'into';
  const range = parseRange(value[key], `${path}.${key}`);
  if (!range.ok) return range;
  if (key === 'before') return { ok: true, value: { before: range.value } };
  if (key === 'after') return { ok: true, value: { after: range.value } };
  if (value.at !== 'start' && value.at !== 'end') return bad(`${path}.at`, 'an into destination needs at: start or end');
  if (value.block !== undefined && !isInt(value.block)) return bad(`${path}.block`, 'block is a non-negative integer');
  return { ok: true, value: { into: range.value, at: value.at, ...(value.block === undefined ? {} : { block: value.block }) } };
}

function parseSemanticStep(value: Record<string, unknown>, op: SemanticStepOp, path: string, guard: { expect?: string }): Parsed<SemanticStep> {
  if (op === 'insertSnippet') {
    const at = parseDestination(value.at, `${path}.at`);
    if (!at.ok) return at;
    if (typeof value.text !== 'string') return bad(`${path}.text`, 'text is a string');
    const text = normalizeText(value.text);
    if (text.trim() === '' || text.length > MAX_SNIPPET) return bad(`${path}.text`, `a snippet is 1 to ${MAX_SNIPPET} characters and not blank`);
    return { ok: true, value: { op, at: at.value, text } };
  }
  const node = parseTarget(value.node, `${path}.node`);
  if (!node.ok) return node;
  if (op === 'reindentBlock') {
    const by = value.by;
    if (by !== undefined && !(typeof by === 'number' && Number.isInteger(by) && Math.abs(by) <= MAX_REINDENT)) return bad(`${path}.by`, `by is an integer from -${MAX_REINDENT} to ${MAX_REINDENT}`);
    return { ok: true, value: { op, node: node.value, ...(by === undefined ? {} : { by: by as number }) } };
  }
  if (op === 'moveNode') {
    const to = parseDestination(value.to, `${path}.to`);
    return to.ok ? { ok: true, value: { op, node: node.value, to: to.value, ...guard } } : to;
  }
  if (op === 'wrapNode') {
    const header = value.header;
    if (typeof header !== 'string' || header.trim() === '' || header.length > MAX_HEADER || /[\r\n]/.test(header)) return bad(`${path}.header`, `a header is one non-empty line of at most ${MAX_HEADER} characters`);
    return { ok: true, value: { op, node: node.value, header, ...guard } };
  }
  return { ok: true, value: { op, node: node.value, ...guard } };
}

function parseStep(value: unknown, index: number, steps: readonly unknown[]): Parsed<Step> {
  const path = `steps[${index}]`;
  if (!isObject(value) || typeof value.op !== 'string') return bad(path, 'a step is an object with an op');
  const expect = value.expect;
  if (expect !== undefined && typeof expect !== 'string') return bad(`${path}.expect`, 'expect is a string');
  const guard = typeof expect === 'string' ? { expect: normalizeText(expect) } : {};
  if (value.op === 'replace') {
    const range = parseTarget(value.range, `${path}.range`);
    if (!range.ok) return range;
    if (typeof value.text !== 'string') return bad(`${path}.text`, 'text is a string');
    return { ok: true, value: { op: 'replace', range: range.value, text: normalizeText(value.text), ...guard } };
  }
  if (value.op === 'move') {
    const range = parseTarget(value.range, `${path}.range`);
    if (!range.ok) return range;
    if (!isInt(value.to)) return bad(`${path}.to`, 'to is a non-negative integer');
    return { ok: true, value: { op: 'move', range: range.value, to: value.to, ...guard } };
  }
  if (value.op === 'select') {
    const anchor = parsePoint(value.anchor, `${path}.anchor`, steps);
    if (!anchor.ok) return anchor;
    if (value.reveal !== undefined && typeof value.reveal !== 'boolean') return bad(`${path}.reveal`, 'reveal is a boolean');
    const reveal = value.reveal === true ? { reveal: true } : {};
    if (value.head === undefined) return { ok: true, value: { op: 'select', anchor: anchor.value, ...reveal } };
    const head = parsePoint(value.head, `${path}.head`, steps);
    return head.ok ? { ok: true, value: { op: 'select', anchor: anchor.value, head: head.value, ...reveal } } : head;
  }
  if (value.op === 'type') {
    if (typeof value.text !== 'string') return bad(`${path}.text`, 'text is a string');
    const text = normalizeText(value.text);
    if (text.length === 0 || text.length > MAX_TYPED) return bad(`${path}.text`, `a type step takes 1 to ${MAX_TYPED} characters: a keystroke, not a paste`);
    return { ok: true, value: { op: 'type', text } };
  }
  if (value.op === 'complete') return { ok: true, value: { op: 'complete' } };
  if (isSemanticOp(value.op)) return parseSemanticStep(value, value.op, path, guard);
  return { ok: false, error: { code: 'unsupported', message: `this editor has no step ${value.op}`, what: `step:${value.op}` } };
}

export function parseApply(params: unknown): Parsed<ApplyParams> {
  if (!isObject(params) || !Array.isArray(params.steps)) return bad('steps', 'apply takes {steps: Step[]}');
  if (params.steps.length > MAX_STEPS) return bad('steps', `a batch holds at most ${MAX_STEPS} steps`);
  const steps: Step[] = [];
  for (const [index, raw] of params.steps.entries()) {
    const step = parseStep(raw, index, params.steps);
    if (!step.ok) return step;
    steps.push(step.value);
  }
  if (steps.length > 1 && steps.some((step) => step.op === 'type' || step.op === 'complete')) return bad('steps', 'a type or complete step is the only step of its call');
  if (steps.some(isSemanticStep) && steps.some((step) => step.op === 'replace' || step.op === 'move')) return bad('steps', 'a node step cannot share a call with a replace or move step');
  if (params.preview !== undefined && typeof params.preview !== 'boolean') return bad('preview', 'preview is a boolean');
  if (params.preview === true && steps.some((step) => step.op === 'type' || step.op === 'complete')) return bad('preview', 'a type or complete step cannot be previewed');
  if (params.budgetMs !== undefined && !(isInt(params.budgetMs) && params.budgetMs <= MAX_TREE_BUDGET_MS)) return bad('budgetMs', `budgetMs is an integer from 0 to ${MAX_TREE_BUDGET_MS}`);
  if (params.at !== undefined && !isInt(params.at)) return bad('at', 'at is a revision: a non-negative integer');
  if (params.stale !== undefined && params.stale !== 'reject' && params.stale !== 'map') return bad('stale', 'stale is reject or map');
  if (params.instance !== undefined && typeof params.instance !== 'string') return bad('instance', 'instance is a string');
  if (params.label !== undefined && (typeof params.label !== 'string' || !LABEL.test(params.label))) return bad('label', 'a label matches /^[a-z0-9][a-z0-9.-]{0,31}$/');
  return {
    ok: true,
    value: {
      steps,
      ...(params.at === undefined ? {} : { at: params.at as number }),
      ...(params.stale === undefined ? {} : { stale: params.stale as 'reject' | 'map' }),
      ...(params.instance === undefined ? {} : { instance: params.instance as string }),
      ...(params.label === undefined ? {} : { label: params.label as string }),
      ...(params.preview === undefined ? {} : { preview: params.preview as boolean }),
      ...(params.budgetMs === undefined ? {} : { budgetMs: params.budgetMs as number }),
    },
  };
}

export function parseRead(params: unknown): Parsed<ReadParams> {
  if (params === undefined || params === null) return { ok: true, value: {} };
  if (!isObject(params)) return bad('', 'read takes {range?}');
  if (params.range === undefined) return { ok: true, value: {} };
  const range = parseRange(params.range, 'range');
  return range.ok ? { ok: true, value: { range: range.value } } : range;
}

export function parseHistory(params: unknown): Parsed<HistoryParams> {
  if (!isObject(params) || (params.direction !== 'undo' && params.direction !== 'redo')) return bad('direction', 'direction is undo or redo');
  return { ok: true, value: { direction: params.direction } };
}

function parseSelRange(value: unknown, path: string): Parsed<SelRange> {
  if (!isObject(value) || !isInt(value.anchor) || !isInt(value.head)) return bad(path, 'a selection is {anchor, head} of non-negative integers');
  return { ok: true, value: { anchor: value.anchor, head: value.head } };
}

export function parseLoad(params: unknown): Parsed<LoadParams> {
  if (!isObject(params) || typeof params.text !== 'string') return bad('text', 'load takes {text: string}');
  if (params.selection === undefined) return { ok: true, value: { text: params.text } };
  const selection = parseSelRange(params.selection, 'selection');
  return selection.ok ? { ok: true, value: { text: params.text, selection: selection.value } } : selection;
}

function parseMarks(value: unknown): Parsed<NonNullable<ViewParams['marks']>> {
  if (!isObject(value)) return bad('marks', 'marks is {set?, clear?}');
  const parsed: { set?: MarkSpec[]; clear?: readonly string[] | 'all' } = {};
  if (value.set !== undefined) {
    if (!Array.isArray(value.set) || value.set.length > MAX_MARKS) return bad('marks.set', `set is an array of at most ${MAX_MARKS} marks`);
    const specs: MarkSpec[] = [];
    for (const [index, raw] of value.set.entries()) {
      const path = `marks.set[${index}]`;
      if (!isObject(raw)) return bad(path, 'a mark is {id, kind, range}');
      if (typeof raw.id !== 'string' || !LABEL.test(raw.id)) return bad(`${path}.id`, 'an id matches /^[a-z0-9][a-z0-9.-]{0,31}$/');
      if (specs.some((spec) => spec.id === raw.id)) return bad(`${path}.id`, 'an id appears once in a set');
      if (typeof raw.kind !== 'string' || !(MARK_KINDS as readonly string[]).includes(raw.kind)) return bad(`${path}.kind`, `kind is ${MARK_KINDS.join(' or ')}`);
      const range = parseRange(raw.range, `${path}.range`);
      if (!range.ok) return range;
      if (raw.hidden !== undefined && typeof raw.hidden !== 'boolean') return bad(`${path}.hidden`, 'hidden is a boolean');
      specs.push({ id: raw.id, kind: raw.kind as MarkKind, range: range.value, ...(raw.hidden === undefined ? {} : { hidden: raw.hidden }) });
    }
    parsed.set = specs;
  }
  if (value.clear !== undefined) {
    if (value.clear === 'all') parsed.clear = 'all';
    else {
      if (!Array.isArray(value.clear) || value.clear.length > MAX_MARKS) return bad('marks.clear', `clear is all or an array of at most ${MAX_MARKS} ids`);
      const ids: string[] = [];
      for (const [index, id] of value.clear.entries()) {
        if (typeof id !== 'string' || !LABEL.test(id)) return bad(`marks.clear[${index}]`, 'an id matches /^[a-z0-9][a-z0-9.-]{0,31}$/');
        ids.push(id);
      }
      parsed.clear = ids;
    }
  }
  return { ok: true, value: parsed };
}

export function parseView(params: unknown): Parsed<ViewParams> {
  if (params === undefined || params === null) return { ok: true, value: {} };
  if (!isObject(params)) return bad('', 'view takes {focus?, at?, instance?, marks?, reveal?}');
  if (params.focus !== undefined && typeof params.focus !== 'boolean') return bad('focus', 'focus is a boolean');
  if (params.at !== undefined && !isInt(params.at)) return bad('at', 'at is a revision: a non-negative integer');
  if (params.instance !== undefined && typeof params.instance !== 'string') return bad('instance', 'instance is a string');
  const value: { -readonly [K in keyof ViewParams]?: ViewParams[K] } = {};
  if (params.focus !== undefined) value.focus = params.focus;
  if (params.at !== undefined) value.at = params.at;
  if (params.instance !== undefined) value.instance = params.instance;
  if (params.marks !== undefined) {
    const marks = parseMarks(params.marks);
    if (!marks.ok) return marks;
    value.marks = marks.value;
  }
  if (params.reveal !== undefined) {
    const reveal = parseRange(params.reveal, 'reveal');
    if (!reveal.ok) return reveal;
    value.reveal = reveal.value;
  }
  return { ok: true, value };
}

const finite = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value);
const short = (value: unknown): value is string => typeof value === 'string' && value.length <= 128;

function parseCompletion(value: unknown): Parsed<CompletionConfig> {
  if (!isObject(value) || typeof value.enabled !== 'boolean') return bad('completion.enabled', 'enabled is a boolean');
  const entries: CompletionEntry[] = [];
  if (value.entries !== undefined) {
    if (!Array.isArray(value.entries)) return bad('completion.entries', 'entries is an array');
    if (value.entries.length > MAX_COMPLETIONS) return bad('completion.entries', `at most ${MAX_COMPLETIONS} entries`);
    for (const [index, raw] of value.entries.entries()) {
      const path = `completion.entries[${index}]`;
      if (!isObject(raw) || typeof raw.label !== 'string' || !COMPLETION_LABEL.test(raw.label)) return bad(`${path}.label`, 'a label is 1 to 64 word characters');
      if (raw.type !== undefined && !(typeof raw.type === 'string' && raw.type.length <= 32)) return bad(`${path}.type`, 'type is a string of at most 32 characters');
      if (raw.detail !== undefined && !(typeof raw.detail === 'string' && raw.detail.length <= 64)) return bad(`${path}.detail`, 'detail is a string of at most 64 characters');
      if (raw.boost !== undefined && !(typeof raw.boost === 'number' && Number.isInteger(raw.boost) && raw.boost >= -99 && raw.boost <= 99)) return bad(`${path}.boost`, 'boost is an integer from -99 to 99');
      entries.push({
        label: raw.label,
        ...(raw.type === undefined ? {} : { type: raw.type as string }),
        ...(raw.detail === undefined ? {} : { detail: raw.detail as string }),
        ...(raw.boost === undefined ? {} : { boost: raw.boost as number }),
      });
    }
  }
  return { ok: true, value: { enabled: value.enabled, entries } };
}

export function parseTree(params: unknown): Parsed<TreeParams> {
  if (params === undefined || params === null) return { ok: true, value: {} };
  if (!isObject(params)) return bad('', 'tree takes {range?, detail?, budgetMs?}');
  const value: { -readonly [K in keyof TreeParams]?: TreeParams[K] } = {};
  if (params.range !== undefined) {
    const range = parseRange(params.range, 'range');
    if (!range.ok) return range;
    value.range = range.value;
  }
  if (params.detail !== undefined) {
    if (params.detail !== 'outline' && params.detail !== 'full') return bad('detail', 'detail is outline or full');
    value.detail = params.detail;
  }
  if (params.budgetMs !== undefined) {
    if (!isInt(params.budgetMs) || params.budgetMs > MAX_TREE_BUDGET_MS) return bad('budgetMs', `budgetMs is an integer from 0 to ${MAX_TREE_BUDGET_MS}`);
    value.budgetMs = params.budgetMs;
  }
  return { ok: true, value };
}

export function parseConfigPatch(params: unknown): Parsed<ConfigPatch> {
  if (!isObject(params)) return bad('', 'configure takes an object');
  const patch: { -readonly [K in keyof ConfigPatch]?: ConfigPatch[K] } = {};
  if (params.language !== undefined) {
    if (typeof params.language !== 'string') return bad('language', 'language is a string');
    patch.language = params.language as Language;
  }
  if (params.wrap !== undefined) {
    if (typeof params.wrap !== 'boolean') return bad('wrap', 'wrap is a boolean');
    patch.wrap = params.wrap;
  }
  if (params.whitespace !== undefined) {
    if (typeof params.whitespace !== 'boolean') return bad('whitespace', 'whitespace is a boolean');
    patch.whitespace = params.whitespace;
  }
  if (params.tabSize !== undefined) {
    if (!isInt(params.tabSize) || params.tabSize < 1 || params.tabSize > 32) return bad('tabSize', 'tabSize is an integer from 1 to 32');
    patch.tabSize = params.tabSize;
  }
  for (const key of ['fontSize', 'lineHeight'] as const) {
    if (params[key] === undefined) continue;
    if (!finite(params[key])) return bad(key, `${key} is a finite number`);
    patch[key] = params[key] as number;
  }
  if (params.completion !== undefined) {
    const completion = parseCompletion(params.completion);
    if (!completion.ok) return completion;
    patch.completion = completion.value;
  }
  if (params.palette !== undefined) {
    if (!isObject(params.palette)) return bad('palette', 'palette is an object');
    const palette: { -readonly [K in keyof Palette]?: Palette[K] } = {};
    if (params.palette.dark !== undefined) {
      if (typeof params.palette.dark !== 'boolean') return bad('palette.dark', 'dark is a boolean');
      palette.dark = params.palette.dark;
    }
    if (params.palette.syntax !== undefined) {
      if (params.palette.syntax !== 'default' && params.palette.syntax !== 'contrast') return bad('palette.syntax', 'syntax is default or contrast');
      palette.syntax = params.palette.syntax;
    }
    for (const key of PALETTE_KEYS) {
      const value = params.palette[key];
      if (value === undefined) continue;
      if (!short(value)) return bad(`palette.${key}`, `${key} is a string of at most 128 characters`);
      palette[key] = value;
    }
    patch.palette = palette;
  }
  return { ok: true, value: patch };
}

export const isLanguage = (value: string): value is Language => (LANGUAGE_NAMES as readonly string[]).includes(value);
export const isSemanticOp = (op: string): op is SemanticStepOp => (SEMANTIC_STEP_OPS as readonly string[]).includes(op);
export const isSemanticStep = (step: Step): step is SemanticStep => isSemanticOp(step.op);
