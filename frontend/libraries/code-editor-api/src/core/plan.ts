import type { DocView } from '../ports/host';
import type { Assoc, Change, EditorError, Point, Pos, Range, SelRange } from '../ports/protocol';
import { mapPos, mapRange } from './changes';

export type ResolvedStep =
  | { readonly op: 'replace'; readonly range: Range; readonly text: string }
  | { readonly op: 'move'; readonly range: Range; readonly to: Pos }
  | { readonly op: 'select'; readonly anchor: Point; readonly head?: Point }
  // edits are in the planned document and may sit anywhere; `landed` names one edit and the part of its insert that is the step's own text (null: where the text it deleted was, as a caret)
  | { readonly op: 'splice'; readonly edits: readonly Change[]; readonly landed: { readonly edit: number; readonly from: number; readonly to: number } | null };

export type Plan = {
  /** individual, sorted, simultaneous, in the coordinates of the document the plan was made against */
  readonly changes: readonly Change[];
  /** per step, where its inserted text sits in the result; null for a select step */
  readonly landed: readonly (Range | null)[];
  /** per step, all it inserts as one range of the result (the hull of its inserts); null when it inserts nothing */
  readonly written: readonly (Range | null)[];
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
  const producer = new Map<number, { entry: Entry; from: number; to: number }>(); // the entry whose insert holds a step's text, and where in it
  let selectAt = -1;
  let seq = 0;
  for (const [step, resolved] of steps.entries()) {
    if (resolved.op === 'select') {
      if (selectAt >= 0) return fail({ code: 'invalid', message: 'a batch holds at most one select step', path: `steps[${step}]` });
      selectAt = step;
      continue;
    }
    if (resolved.op === 'splice') {
      const own: (Entry | null)[] = [];
      for (const edit of resolved.edits) {
        if (!isPos(edit.from, length) || !isPos(edit.to, length) || edit.from > edit.to) {
          return fail({ code: 'out-of-range', message: `step ${step} names a range outside the document (length ${length})`, length, step });
        }
        const added: Entry | null = edit.from === edit.to && edit.insert === '' ? null : { from: edit.from, to: edit.to, insert: edit.insert, step, seq: (seq += 1) };
        if (added) entries.push(added);
        own.push(added);
      }
      const { landed } = resolved;
      const chosen = landed ? own[landed.edit] : null;
      if (landed && (!chosen || landed.from < 0 || landed.from > landed.to || landed.to > chosen.insert.length)) {
        return fail({ code: 'internal', message: `step ${step} names text that its edits do not insert` });
      }
      if (landed && chosen) producer.set(step, { entry: chosen, from: landed.from, to: landed.to });
      else if (resolved.edits.length > 0) {
        const first = Math.min(...resolved.edits.map((edit) => edit.from));
        idle.push({ step, range: { from: first, to: first } });
      }
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
        producer.set(step, { entry: added, from: 0, to: added.insert.length });
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
    producer.set(step, { entry: added, from: 0, to: added.insert.length });
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
  for (const [step, { entry, from, to }] of producer) {
    const start = startOf.get(entry) as number;
    landed[step] = { from: start + from, to: start + to };
  }
  for (const { step, range } of idle) landed[step] = mapRange(range, changes);
  const written: (Range | null)[] = steps.map(() => null);
  for (const entry of entries) {
    if (entry.insert === '') continue;
    const start = startOf.get(entry) as number;
    const soFar = written[entry.step];
    written[entry.step] = { from: Math.min(soFar?.from ?? start, start), to: Math.max(soFar?.to ?? 0, start + entry.insert.length) };
  }
  const select = selectAt < 0 ? null : resolveSelect(steps[selectAt] as Extract<ResolvedStep, { op: 'select' }>, selectAt, changes, landed, length);
  if (select !== null && 'code' in select) return fail(select);
  return { ok: true, plan: { changes, landed, written, select } };
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
