import type { Change, Mark, MarkSpec } from '../ports/protocol';
import { mapRange } from './changes';

export const MAX_MARKS = 8;

const markOf = (spec: MarkSpec): Mark => ({ id: spec.id, kind: spec.kind, range: { from: spec.range.from, to: spec.range.to }, ...(spec.hidden ? { hidden: true as const } : {}) });

/** A spec with the id of a mark replaces it where it stands; any other is added at the end. */
export function setMarks(list: readonly Mark[], specs: readonly MarkSpec[]): Mark[] {
  const next = [...list];
  for (const spec of specs) {
    const at = next.findIndex((mark) => mark.id === spec.id);
    if (at < 0) next.push(markOf(spec));
    else next[at] = markOf(spec);
  }
  return next;
}

export const clearMarks = (list: readonly Mark[], ids: readonly string[] | 'all'): Mark[] => (ids === 'all' ? [] : list.filter((mark) => !ids.includes(mark.id)));

/**
 * Where the marks go when `changes` (individual and simultaneous) apply: text typed at the start or the end of a mark stays outside it, a mark that had text and lost all of it is
 * dropped, and a mark that was empty (a blank line) is kept. The engine and the host both map their copy with this, so they agree without a message.
 */
export function mapMarks(list: readonly Mark[], changes: readonly Change[]): Mark[] {
  return list.flatMap((mark) => {
    const range = mapRange(mark.range, changes);
    return mark.range.from < mark.range.to && range.from === range.to ? [] : [{ ...mark, range }];
  });
}
