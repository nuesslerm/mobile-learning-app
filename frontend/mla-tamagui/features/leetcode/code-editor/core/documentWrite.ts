import { diffChange, normalizeText, type Point, type Step } from '@lib/code-editor-api';

import type { Selection } from './types';

/**
 * The steps that turn the editor's `current` text into `next` as one hunk (the common start and end are kept, so what the person
 * did elsewhere, their undo history and the marks outside the hunk survive), and that leave the selection where `next` says.
 * The select points are chosen by where each position falls against the hunk: before it, a position of the old text; inside or
 * at the edge of what the hunk inserts, a point in that insert; after it, the old position the batch maps to the wanted one.
 */
export function planWrite(current: string, next: string, selection?: Selection): Step[] {
  const text = normalizeText(next);
  const hunk = diffChange(current, text);
  const steps: Step[] = hunk ? [{ op: 'replace', range: { from: hunk.from, to: hunk.to }, text: hunk.insert }] : [];
  if (!selection) return steps;

  const place = (wanted: number): Point => {
    const pos = Math.max(0, Math.min(wanted, text.length));
    if (!hunk || pos < hunk.from) return pos;
    if (pos <= hunk.from + hunk.insert.length) return { step: 0, edge: 'start', offset: pos - hunk.from };
    return pos - (hunk.insert.length - (hunk.to - hunk.from));
  };
  const anchor = place(selection.start);
  steps.push(selection.start === selection.end ? { op: 'select', anchor } : { op: 'select', anchor, head: place(selection.end) });
  return steps;
}
