import type { EditorClient, EditorErrorCode, Range, Revision } from '@lib/code-editor-api';

export const ERROR_MARK_ID = 'run-error';

/** The editor as a run found it: the text the run is made on, and the instance and revision a later mark is placed at. One read, so a line number always names a line of that text. */
export type ErrorCheckpoint = { readonly text: string; readonly revision: Revision; readonly instance: string };

/** Where a runner says the error was raised: 1-based, and the column is not drawn. */
export type ErrorPlace = { readonly line: number; readonly column?: number };

export type ErrorJump =
  | { readonly kind: 'shown'; readonly line: number; readonly range: Range }
  | { readonly kind: 'no-location' }
  | { readonly kind: 'outside'; readonly line: number }
  | { readonly kind: 'gone'; readonly line: number }
  | { readonly kind: 'refused'; readonly code: EditorErrorCode };

/** The content of line `line` (1-based): from its first non-blank character to its end. A blank line is an empty range at its end; null for a line the text does not have. */
export function errorLineRange(text: string, line: number): Range | null {
  if (!Number.isInteger(line) || line < 1) return null;
  let from = 0;
  for (let number = 1; number < line; number += 1) {
    const newline = text.indexOf('\n', from);
    if (newline < 0) return null;
    from = newline + 1;
  }
  const newline = text.indexOf('\n', from);
  const to = newline < 0 ? text.length : newline;
  const content = /\S/.exec(text.slice(from, to));
  return { from: content ? from + content.index : to, to };
}

/** Hello and the text in one round trip: null when the editor is not ready or either call fails. A page that restarts between the two is refused later, as an instance mismatch. */
export async function checkpointOf(client: EditorClient): Promise<ErrorCheckpoint | null> {
  const [hello, read] = await Promise.all([client.hello(), client.getText()]);
  if (!hello.ok || !hello.value.ready || !read.ok) return null;
  return { text: read.value.text, revision: read.revision, instance: hello.value.instance };
}

/** The checkpoint once `after` has settled (the writes still queued), or null when `ms` pass first: a silent editor must not hold a run back. */
export function checkpointWithin(client: EditorClient, ms: number, after: Promise<unknown> = Promise.resolve()): Promise<ErrorCheckpoint | null> {
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve(null), ms);
    void after
      .then(() => checkpointOf(client))
      .then((checkpoint) => {
        clearTimeout(timer);
        resolve(checkpoint);
      });
  });
}

/**
 * Marks the line a run reported and scrolls it into view, in one atomic call placed at the checkpoint's revision: the engine carries the
 * range across what was typed since. The selection and the focus are not touched, so the keyboard does not rise. Nothing is called when
 * there is no line to show.
 */
export async function showErrorLine(client: EditorClient, checkpoint: ErrorCheckpoint, place: ErrorPlace | null | undefined): Promise<ErrorJump> {
  if (!place) return { kind: 'no-location' };
  const range = errorLineRange(checkpoint.text, place.line);
  if (!range) return { kind: 'outside', line: place.line };
  const reply = await client.setMarks([{ id: ERROR_MARK_ID, kind: 'error', range }], { at: checkpoint.revision, instance: checkpoint.instance, reveal: range });
  if (reply.ok) return { kind: 'shown', line: place.line, range: reply.value.marks.find((mark) => mark.id === ERROR_MARK_ID)?.range ?? range };
  return reply.error.code === 'range-lost' ? { kind: 'gone', line: place.line } : { kind: 'refused', code: reply.error.code };
}

/** Takes the error mark away and leaves every other mark; the refusal code, or null when it went. */
export async function clearErrorLine(client: EditorClient): Promise<EditorErrorCode | null> {
  const reply = await client.clearMarks([ERROR_MARK_ID]);
  return reply.ok ? null : reply.error.code;
}
