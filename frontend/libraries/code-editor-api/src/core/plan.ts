import type { DocView } from '../ports/host';
import type { Assoc, Change, EditorError, Point, Pos, Range, SelRange } from '../ports/protocol';
import { mapPos, mapRange } from './changes';

export type ResolvedStep =
  | { readonly op: 'replace'; readonly range: Range; readonly text: string }
  | { readonly op: 'move'; readonly range: Range; readonly to: Pos }
  | { readonly op: 'select'; readonly anchor: Point; readonly head?: Point };

export type Plan = {
  /** individual, sorted, simultaneous, in the coordinates of the document the plan was made against */
  readonly changes: readonly Change[];
  /** per step, where its inserted text sits in the result; null for a select step */
  readonly landed: readonly (Range | null)[];
  /** the explicit selection in result coordinates, null when the host's default mapping applies */
  readonly select: SelRange | null;
};

type Entry = { readonly from: number; readonly to: number; readonly insert: string; readonly step: number; readonly seq: number };
type Planned = { ok: true; plan: Plan } | { ok: false; error: EditorError };

const isPos = (value: number, length: number) => Number.isInteger(value) && value >= 0 && value <= length;
const fail = (error: EditorError): Planned => ({ ok: false, error });
const rank = (entry: Entry) => (entry.to > entry.from ? 1 : 0);

export function planBatch(doc: DocView, steps: readonly ResolvedStep[]): Planned {
  const { length } = doc;
  const entries: Entry[] = [];
  const idle: { step: number; range: Range }[] = [];
  const producer = new Map<number, Entry>();
  let selectAt = -1;
  let seq = 0;
  for (const [step, resolved] of steps.entries()) {
    if (resolved.op === 'select') {
      if (selectAt >= 0) return fail({ code: 'invalid', message: 'a batch holds at most one select step', path: `steps[${step}]` });
      selectAt = step;
      continue;
    }
    const range = resolved.range;
    if (!isPos(range.from, length) || !isPos(range.to, length) || range.from > range.to) {
      return fail({ code: 'out-of-range', message: `step ${step} names a range outside the document (length ${length})`, length, step });
    }
    if (resolved.op === 'replace') {
      if (range.from === range.to && resolved.text === '') idle.push({ step, range });
      else {
        const added: Entry = { from: range.from, to: range.to, insert: resolved.text, step, seq: (seq += 1) };
        entries.push(added);
        producer.set(step, added);
      }
      continue;
    }
    if (!isPos(resolved.to, length)) return fail({ code: 'out-of-range', message: `step ${step} moves to a position outside the document (length ${length})`, length, step });
    if (range.from < resolved.to && resolved.to < range.to) return fail({ code: 'invalid', message: 'a range cannot move into itself', path: `steps[${step}].to` });
    if (range.from === range.to || resolved.to === range.from || resolved.to === range.to) {
      idle.push({ step, range });
      continue;
    }
    entries.push({ from: range.from, to: range.to, insert: '', step, seq: (seq += 1) });
    const added: Entry = { from: resolved.to, to: resolved.to, insert: doc.slice(range.from, range.to), step, seq: (seq += 1) };
    entries.push(added);
    producer.set(step, added);
  }
  entries.sort((a, b) => a.from - b.from || rank(a) - rank(b) || a.step - b.step || a.seq - b.seq);
  for (let i = 1; i < entries.length; i += 1) {
    const previous = entries[i - 1] as Entry;
    const current = entries[i] as Entry;
    if (previous.to > current.from) {
      return fail({ code: 'overlap', message: `steps ${previous.step} and ${current.step} touch the same text`, steps: [previous.step, current.step] });
    }
  }
  const changes: Change[] = [];
  const startOf = new Map<Entry, number>();
  let delta = 0;
  for (const entry of entries) {
    startOf.set(entry, entry.from + delta);
    changes.push({ from: entry.from, to: entry.to, insert: entry.insert });
    delta += entry.insert.length - (entry.to - entry.from);
  }
  const landed: (Range | null)[] = steps.map(() => null);
  for (const [step, entry] of producer) {
    const start = startOf.get(entry) as number;
    landed[step] = { from: start, to: start + entry.insert.length };
  }
  for (const { step, range } of idle) landed[step] = mapRange(range, changes);
  const select = selectAt < 0 ? null : resolveSelect(steps[selectAt] as Extract<ResolvedStep, { op: 'select' }>, selectAt, changes, landed, length);
  if (select !== null && 'code' in select) return fail(select);
  return { ok: true, plan: { changes, landed, select } };
}

function resolveSelect(step: Extract<ResolvedStep, { op: 'select' }>, index: number, changes: readonly Change[], landed: readonly (Range | null)[], length: number): SelRange | EditorError {
  const head = step.head ?? step.anchor;
  const bothBare = typeof step.anchor === 'number' && typeof head === 'number' && step.anchor !== head;
  const bare = (value: number, assoc: Assoc): number | EditorError =>
    isPos(value, length) ? mapPos(value, changes, assoc) : { code: 'out-of-range', message: `the selection names a position outside the document (length ${length})`, length, step: index };
  const point = (value: Point, other: Point): number | EditorError => {
    if (typeof value === 'number') {
      const assoc: Assoc = bothBare && typeof other === 'number' && value > other ? -1 : 1;
      return bare(value, assoc);
    }
    if ('at' in value) return bare(value.at, value.assoc);
    const target = landed[value.step];
    if (!target) return { code: 'invalid', message: `a point refers to step ${value.step}, which inserts nothing`, path: `steps[${index}]` };
    const pos = (value.edge === 'end' ? target.to : target.from) + (value.offset ?? 0);
    return Number.isInteger(pos) && pos >= target.from && pos <= target.to ? pos : { code: 'out-of-range', message: `a point leaves the text step ${value.step} inserted`, length, step: value.step };
  };
  const anchor = point(step.anchor, head);
  if (typeof anchor !== 'number') return anchor;
  const resolvedHead = step.head === undefined ? anchor : point(head, step.anchor);
  if (typeof resolvedHead !== 'number') return resolvedHead;
  const crossed = typeof step.anchor === 'number' && typeof step.head === 'number' && step.anchor < step.head !== anchor < resolvedHead && anchor !== resolvedHead;
  return crossed ? { anchor: Math.max(anchor, resolvedHead), head: Math.max(anchor, resolvedHead) } : { anchor, head: resolvedHead };
}
