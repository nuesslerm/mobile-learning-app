import { applyChanges, invertChanges, mapSelRange } from '../core/changes';
import type { Change, SelRange } from '../ports/protocol';

// A model of @codemirror/commands' undo history, as faithful as the differential test can prove: events carry the selection before them,
// the selections seen after them, and typing joins the previous event while it is quick, adjacent and nothing selected in between.

const NEW_GROUP_DELAY = 500;
const JOINABLE = /^(input\.type|delete)($|\.)/;
const SELECT = /^select($|\.)/;

type HistoryEvent = {
  /** the changes that carry the document to the other side of this event, in the coordinates of the document they apply to; null for a selection-only event */
  readonly apply: readonly Change[] | null;
  readonly startSelection: SelRange | null;
  readonly selectionsAfter: readonly SelRange[];
  /** the whole event as one forward hunk, kept so later typing can join into it */
  readonly typed: Change | null;
};
export type HistoryState = {
  readonly done: readonly HistoryEvent[];
  readonly undone: readonly HistoryEvent[];
  readonly prevTime: number | null;
  readonly prevUserEvent: string | null;
};

export const emptyHistory: HistoryState = { done: [], undone: [], prevTime: null, prevUserEvent: null };

const isolate = (state: HistoryState): HistoryState => (state.prevTime === null ? state : { ...state, prevTime: null, prevUserEvent: null });
const sameSelection = (a: SelRange, b: SelRange) => a.anchor === b.anchor && a.head === b.head;
const sameShape = (a: SelRange, b: SelRange) => (a.anchor === a.head) === (b.anchor === b.head);

export type RecordedChange = {
  readonly forward: readonly Change[];
  readonly before: string;
  readonly after: string;
  readonly startSelection: SelRange;
  readonly time: number;
  readonly userEvent: string;
  /** `isolateHistory.of('full')`: nothing joins into this event or after it */
  readonly isolated: boolean;
};

function composedHunk(first: Change, next: Change, after: string): Change {
  const grown = first.from + first.insert.length;
  const from = Math.min(first.from, next.from);
  const to = next.to > grown ? first.to + (next.to - grown) : first.to;
  const delta = first.insert.length - (first.to - first.from) + (next.insert.length - (next.to - next.from));
  return { from, to, insert: after.slice(from, from + (to - from) + delta) };
}

export function recordChange(history: HistoryState, change: RecordedChange): HistoryState {
  const state = change.isolated ? isolate(history) : history;
  const last = state.done[state.done.length - 1];
  const single = change.forward.length === 1 ? (change.forward[0] as Change) : null;
  const adjacent = last?.apply ? last.apply.some((c) => change.forward.some((f) => f.to >= c.from && f.from <= c.to)) : false;
  const joins =
    last !== undefined && last.apply !== null && last.apply.length > 0 && last.typed !== null && single !== null && JOINABLE.test(change.userEvent) && last.selectionsAfter.length === 0 &&
    state.prevTime !== null && change.time - state.prevTime < NEW_GROUP_DELAY && adjacent;
  let done: readonly HistoryEvent[];
  if (joins && last && last.typed && single) {
    const hunk = composedHunk(last.typed, single, change.after);
    const origin = applyChanges(change.before, last.apply ?? []);
    done = [...state.done.slice(0, -1), { apply: invertChanges(origin, [hunk]), startSelection: last.startSelection, selectionsAfter: [], typed: hunk }];
  } else {
    done = [...state.done, { apply: invertChanges(change.before, change.forward), startSelection: change.startSelection, selectionsAfter: [], typed: single }];
  }
  const next: HistoryState = { done, undone: [], prevTime: change.time, prevUserEvent: change.userEvent };
  return change.isolated ? isolate(next) : next;
}

/** A selection-only transaction: CodeMirror files the selection that was current before it under the last event. */
export function recordSelection(history: HistoryState, before: SelRange, time: number, userEvent: string): HistoryState {
  const last = history.done[history.done.length - 1];
  const seen = last?.selectionsAfter ?? [];
  const latest = seen[seen.length - 1];
  if (latest && history.prevTime !== null && time - history.prevTime < NEW_GROUP_DELAY && userEvent === history.prevUserEvent && SELECT.test(userEvent) && sameShape(latest, before)) return history;
  let done: readonly HistoryEvent[];
  if (!last) done = [{ apply: null, startSelection: null, selectionsAfter: [before], typed: null }];
  else if (latest && sameSelection(latest, before)) done = history.done;
  else done = [...history.done.slice(0, -1), { ...last, selectionsAfter: [...seen, before] }];
  return { done, undone: history.undone, prevTime: time, prevUserEvent: userEvent };
}

export type Popped = { readonly apply: readonly Change[]; readonly selection: SelRange; readonly next: HistoryState };

/** Undo (`done`) or redo (`undone`): the changes to apply to `text`, the selection to restore, and the history that remains. */
export function pop(history: HistoryState, side: 'done' | 'undone', text: string, current: SelRange): Popped | null {
  const branch = side === 'done' ? history.done : history.undone;
  const event = branch[branch.length - 1];
  if (!event || !event.apply || !event.startSelection) return null;
  const inverse = invertChanges(text, event.apply);
  const mirrored: HistoryEvent = {
    apply: inverse,
    startSelection: event.selectionsAfter[0] ?? mapSelRange(event.startSelection, inverse, 1) ?? current,
    selectionsAfter: [],
    typed: null,
  };
  const rest = branch.slice(0, -1);
  const next: HistoryState =
    side === 'done' ? { done: rest, undone: [...history.undone, mirrored], prevTime: null, prevUserEvent: null } : { done: [...history.done, mirrored], undone: rest, prevTime: null, prevUserEvent: null };
  return { apply: event.apply, selection: event.startSelection, next };
}
