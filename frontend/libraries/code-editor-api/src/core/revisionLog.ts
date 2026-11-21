import type { Change, Range, Revision } from '../ports/protocol';
import { lengthDelta, mapPos, mapRange } from './changes';

type Entry = { readonly changes: readonly Change[]; readonly lengthAfter: number };

export type RevisionLog = {
  head(): Revision;
  /** the oldest revision an `at` can still be mapped from */
  floor(): Revision;
  /** the document length at `revision`, or null when it is outside the log */
  lengthAt(revision: Revision): number | null;
  push(changes: readonly Change[]): Revision;
  reset(revision: Revision, length: number): void;
  mapPos(pos: number, from: Revision, assoc: -1 | 1): number;
  mapRange(range: Range, from: Revision): Range;
};

export function createRevisionLog(initialRevision: Revision, initialLength: number, depth: number): RevisionLog {
  let floor = initialRevision;
  let baseLength = initialLength;
  let entries: Entry[] = [];
  const head = () => floor + entries.length;
  const lengthAt = (revision: Revision): number | null => {
    if (!Number.isInteger(revision) || revision < floor || revision > head()) return null;
    return revision === floor ? baseLength : (entries[revision - floor - 1]?.lengthAfter ?? null);
  };
  const since = (from: Revision): Change[][] => entries.slice(Math.max(0, from - floor)).map((entry) => [...entry.changes]);
  return {
    head,
    floor: () => floor,
    lengthAt,
    push(changes) {
      entries.push({ changes, lengthAfter: (lengthAt(head()) ?? 0) + lengthDelta(changes) });
      while (entries.length > depth) {
        baseLength = entries[0]?.lengthAfter ?? baseLength;
        entries.shift();
        floor += 1;
      }
      return head();
    },
    reset(revision, length) {
      floor = revision;
      baseLength = length;
      entries = [];
    },
    mapPos(pos, from, assoc) {
      return since(from).reduce((at, changes) => mapPos(at, changes, assoc), pos);
    },
    mapRange(range, from) {
      return since(from).reduce((at, changes) => mapRange(at, changes), range);
    },
  };
}
