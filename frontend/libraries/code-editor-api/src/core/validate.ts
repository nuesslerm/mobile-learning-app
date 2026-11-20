import type {
  ApplyParams, ConfigPatch, EditorError, HistoryParams, Language, LoadParams, Palette, Point, Range, ReadParams, SelRange, Step, ViewParams,
} from '../ports/protocol';
import { normalizeText } from './changes';

export type Parsed<T> = { readonly ok: true; readonly value: T } | { readonly ok: false; readonly error: EditorError };

const MAX_STEPS = 10_000;
const LABEL = /^[a-z0-9][a-z0-9.-]{0,31}$/;
const LANGUAGES: readonly string[] = ['python', 'javascript', 'cpp'];
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
    if (value.head === undefined) return { ok: true, value: { op: 'select', anchor: anchor.value } };
    const head = parsePoint(value.head, `${path}.head`, steps);
    return head.ok ? { ok: true, value: { op: 'select', anchor: anchor.value, head: head.value } } : head;
  }
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

export function parseView(params: unknown): Parsed<ViewParams> {
  if (params === undefined || params === null) return { ok: true, value: {} };
  if (!isObject(params) || (params.focus !== undefined && typeof params.focus !== 'boolean')) return bad('focus', 'focus is a boolean');
  return { ok: true, value: params.focus === undefined ? {} : { focus: params.focus } };
}

const finite = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value);
const short = (value: unknown): value is string => typeof value === 'string' && value.length <= 128;

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
  if (params.tabSize !== undefined) {
    if (!isInt(params.tabSize) || params.tabSize < 1 || params.tabSize > 32) return bad('tabSize', 'tabSize is an integer from 1 to 32');
    patch.tabSize = params.tabSize;
  }
  for (const key of ['fontSize', 'lineHeight'] as const) {
    if (params[key] === undefined) continue;
    if (!finite(params[key])) return bad(key, `${key} is a finite number`);
    patch[key] = params[key] as number;
  }
  if (params.palette !== undefined) {
    if (!isObject(params.palette)) return bad('palette', 'palette is an object');
    const palette: { -readonly [K in keyof Palette]?: Palette[K] } = {};
    if (params.palette.dark !== undefined) {
      if (typeof params.palette.dark !== 'boolean') return bad('palette.dark', 'dark is a boolean');
      palette.dark = params.palette.dark;
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

export const isLanguage = (value: string): value is Language => LANGUAGES.includes(value);
