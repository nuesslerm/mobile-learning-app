import type { EditorClient, Range, Reply, Revision, ViewValue } from '@lib/code-editor-api';

/** The one mark a voice or plan edit leaves: the text it changed. A later edit replaces it. */
export const EDIT_MARK_ID = 'edit';

/**
 * Marks the span an edit landed in. `landed` is in the document the edit produced, `revision` is that document's revision and
 * `instance` the editor it belongs to, so a keystroke that arrives before this call maps the mark to the same text and a page that
 * restarted refuses it.
 */
export function markEdit(client: EditorClient, landed: Range, revision: Revision, instance: string): Promise<Reply<ViewValue>> {
  return client.setMarks([{ id: EDIT_MARK_ID, kind: 'edit', range: landed }], { at: revision, instance });
}

/** Shows or hides the edit mark where the text has carried it; the mark is kept either way, and null means there was none. */
export async function setEditMarkHidden(client: EditorClient, hidden: boolean): Promise<Reply<ViewValue> | null> {
  const read = await client.marks();
  if (!read.ok) return read;
  const mark = read.value.marks.find((held) => held.id === EDIT_MARK_ID);
  if (!mark) return null;
  return client.setMarks([{ id: mark.id, kind: mark.kind, range: mark.range, hidden }], { at: read.revision });
}

export const clearEditMark = (client: EditorClient): Promise<Reply<ViewValue>> => client.clearMarks([EDIT_MARK_ID]);
