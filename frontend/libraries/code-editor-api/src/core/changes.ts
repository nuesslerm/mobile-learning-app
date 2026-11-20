import type { Assoc, Change, Range, SelRange } from '../ports/protocol';

/** `changes` are individual and simultaneous: sorted, non-overlapping, every position in the document they apply to. */
export function applyChanges(text: string, changes: readonly Change[]): string {
  let out = '';
  let at = 0;
  for (const change of changes) {
    out += text.slice(at, change.from) + change.insert;
    at = change.to;
  }
  return out + text.slice(at);
}

export function lengthDelta(changes: readonly Change[]): number {
  let delta = 0;
  for (const change of changes) delta += change.insert.length - (change.to - change.from);
  return delta;
}

// the same rule as CodeMirror's ChangeSet.mapPos, fed the individual change list (a merged list mis-maps boundaries)
export function mapPos(pos: number, changes: readonly Change[], assoc: Assoc): number {
  let delta = 0;
  for (const change of changes) {
    if (change.from > pos) break;
    if (change.to > pos || (change.to === pos && assoc < 0 && change.to === change.from)) {
      return pos === change.from || assoc < 0 ? change.from + delta : change.from + delta + change.insert.length;
    }
    delta += change.insert.length - (change.to - change.from);
  }
  return pos + delta;
}

export function mapRange(range: Range, changes: readonly Change[]): Range {
  if (range.from === range.to) {
    const at = mapPos(range.from, changes, 1);
    return { from: at, to: at };
  }
  const from = mapPos(range.from, changes, 1);
  return { from, to: Math.max(from, mapPos(range.to, changes, -1)) };
}

/**
 * Where a selection goes when `changes` apply: CodeMirror's rule (an empty range stays before text inserted at it, a non-empty one stays
 * inward), except that a range wholly inside replaced text collapses to a caret after the replacement instead of turning inside out.
 */
export function mapSelRange(selection: SelRange, changes: readonly Change[], assoc: Assoc = -1): SelRange {
  if (selection.anchor === selection.head) {
    const at = mapPos(selection.head, changes, assoc);
    return { anchor: at, head: at };
  }
  const forward = selection.anchor < selection.head;
  const low = mapPos(Math.min(selection.anchor, selection.head), changes, 1);
  const high = mapPos(Math.max(selection.anchor, selection.head), changes, -1);
  if (low > high) return { anchor: low, head: low };
  return forward ? { anchor: low, head: high } : { anchor: high, head: low };
}

/** The changes that undo `changes` once applied to `before`, in the coordinates of the document they produced. */
export function invertChanges(before: string, changes: readonly Change[]): Change[] {
  const inverse: Change[] = [];
  let delta = 0;
  for (const change of changes) {
    const from = change.from + delta;
    inverse.push({ from, to: from + change.insert.length, insert: before.slice(change.from, change.to) });
    delta += change.insert.length - (change.to - change.from);
  }
  return inverse;
}

/** One hunk between two texts: the common prefix and suffix are kept. */
export function diffChange(before: string, after: string): Change | null {
  if (before === after) return null;
  const shortest = Math.min(before.length, after.length);
  let start = 0;
  while (start < shortest && before.charCodeAt(start) === after.charCodeAt(start)) start += 1;
  let end = 0;
  while (end < shortest - start && before.charCodeAt(before.length - 1 - end) === after.charCodeAt(after.length - 1 - end)) end += 1;
  return { from: start, to: before.length - end, insert: after.slice(start, after.length - end) };
}

export function normalizeText(text: string): string {
  return text.replace(/\r\n?/g, '\n');
}
