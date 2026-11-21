import { StateEffect, StateField, type Text } from '@codemirror/state';
import { Decoration, EditorView, type DecorationSet } from '@codemirror/view';
import { mapMarks, type Change, type Mark } from '@lib/code-editor-api';

type MarkState = { readonly marks: readonly Mark[]; readonly decorations: DecorationSet };

/** The typed API's marks as the host was last told them, replacing the whole list. */
export const setMarksEffect = StateEffect.define<readonly Mark[]>();

const errorLine = Decoration.line({ class: 'cm-run-error-line' });
const errorText = Decoration.mark({ class: 'cm-run-error' });
const editText = Decoration.mark({ class: 'cm-diff-change' });

/** An error mark tints every line it touches and underlines its text; an edit mark is the diff highlight; a hidden mark is listed and not drawn. */
function decorate(doc: Text, marks: readonly Mark[]): DecorationSet {
  const drawn = [];
  for (const { kind, range, hidden } of marks) {
    if (hidden) continue;
    const from = Math.min(range.from, doc.length);
    const to = Math.min(range.to, doc.length);
    if (kind === 'edit') {
      if (to > from) drawn.push(editText.range(from, to));
      continue;
    }
    for (let line = doc.lineAt(from); ; line = doc.line(line.number + 1)) {
      drawn.push(errorLine.range(line.from));
      if (line.to >= to) break;
    }
    if (to > from) drawn.push(errorText.range(from, to));
  }
  return Decoration.set(drawn, true);
}

/**
 * The host's copy of the engine's marks. It maps them through each transaction's individual changes with the library's own `mapMarks`, so
 * it agrees with the engine without a message, and it never dispatches: the engine pushes a new list from a request handler only.
 */
export const marksField = StateField.define<MarkState>({
  create: () => ({ marks: [], decorations: Decoration.none }),
  update(value, tr) {
    let marks = value.marks;
    let changed = false;
    if (tr.docChanged && marks.length > 0) {
      const changes: Change[] = [];
      tr.changes.iterChanges((from, to, _fromB, _toB, text) => changes.push({ from, to, insert: text.toString() }), true);
      marks = mapMarks(marks, changes);
      changed = true;
    }
    for (const effect of tr.effects) {
      if (!effect.is(setMarksEffect)) continue;
      marks = effect.value;
      changed = true;
    }
    return changed ? { marks, decorations: decorate(tr.newDoc, marks) } : value;
  },
  provide: (field) => EditorView.decorations.from(field, (value) => value.decorations),
});
