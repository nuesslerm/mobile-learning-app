import { diffChange, normalizeText, type EditorClient, type Point, type Step } from '@lib/code-editor-api';

import type { Selection } from '../../domain/editor/types';

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

export type WriteRequest = {
  /** the text and selection to write, given what the editor holds when the attempt runs (so a retry plans against the latest) */
  readonly next: (current: { text: string; selection: Selection }) => { text: string; selection?: Selection };
  readonly label: string;
};

export type WriteOutcome =
  | { readonly landed: true; readonly text: string }
  | { readonly landed: false; readonly code: string; readonly editorText: string | null };

const TRIES = 3;

/**
 * Reads the editor, plans one hunk and applies it at the revision it read. A keystroke that lands in between makes the call stale
 * and the write is planned again, up to three times. When it does not land, `editorText` is what the editor holds after the last
 * attempt (read again, not the text of an earlier attempt, which a keystroke has already moved past), or null when the editor
 * cannot be read. `stillLive` is asked before each attempt; null is returned when it says no.
 */
export async function writeThrough(client: EditorClient, request: WriteRequest, stillLive: () => boolean = () => true): Promise<WriteOutcome | null> {
  let code = 'none';
  for (let attempt = 0; attempt < TRIES; attempt += 1) {
    if (!stillLive()) return null;
    const read = await client.getText();
    if (!read.ok) {
      code = read.error.code;
      break;
    }
    const range = read.value.selection.ranges[read.value.selection.main] ?? { anchor: 0, head: 0 };
    const next = request.next({
      text: read.value.text,
      selection: { start: Math.min(range.anchor, range.head), end: Math.max(range.anchor, range.head) },
    });
    const steps = planWrite(read.value.text, next.text, next.selection);
    if (steps.length === 0) return { landed: true, text: normalizeText(next.text) };
    const reply = await client.transact(steps, { at: read.revision, label: request.label });
    if (reply.ok) return { landed: true, text: normalizeText(next.text) };
    code = reply.error.code;
    if (reply.error.code !== 'stale') break;
  }
  if (!stillLive()) return null;
  const fresh = await client.getText();
  return { landed: false, code, editorText: fresh.ok ? fresh.value.text : null };
}
