import { closeBrackets } from '@codemirror/autocomplete';
import { history, insertNewlineAndIndent, undo } from '@codemirror/commands';
import { EditorState, type Extension } from '@codemirror/state';
import { indentUnit } from '@codemirror/language';
import { EditorView } from '@codemirror/view';
import { LANGUAGE_NAMES, type Language } from '@lib/code-editor-api';
import { afterEach, describe, expect, it } from 'vitest';

import { LANGUAGE_PROFILES } from '../../core/languageProfile';
import { languageCompartment, languageExtension } from '../languageExtensions';

const views: EditorView[] = [];
afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
});

function mount(language: Language, doc: string, selection?: { anchor: number; head?: number }) {
  const extensions: Extension[] = [history(), closeBrackets(), indentUnit.of('    '), EditorState.tabSize.of(4), languageCompartment.of(languageExtension(language))];
  const view = new EditorView({ state: EditorState.create({ doc, selection, extensions }), parent: document.body.appendChild(document.createElement('div')) });
  views.push(view);
  return view;
}

const lineComment = (view: EditorView): string | undefined => {
  const tokens = view.state.languageDataAt<{ line?: string }>('commentTokens', 0);
  return tokens[0]?.line;
};

const OPENERS: Record<Language, string> = { python: 'if x:', javascript: 'function f() {', cpp: 'int main() {' };

describe('the language compartment', () => {
  it('given an editor on Python holding text, a selection and one edit, when the compartment is reconfigured to JavaScript, then the text and selection are unchanged, undo still reverts the edit, and the line comment is //', () => {
    // Given Python text with a selection and one edit made
    const view = mount('python', 'abc\ndef', { anchor: 1, head: 5 });
    view.dispatch({ changes: { from: 0, insert: 'x' }, userEvent: 'input.type' });
    const before = { text: view.state.doc.toString(), selection: view.state.selection.main.toJSON() };
    expect(lineComment(view)).toBe('#');

    // When the compartment is reconfigured
    view.dispatch({ effects: languageCompartment.reconfigure(languageExtension('javascript')) });

    // Then nothing of the document moved, the grammar is the new one, and the edit is still undoable
    expect({ text: view.state.doc.toString(), selection: view.state.selection.main.toJSON() }).toEqual(before);
    expect(lineComment(view)).toBe('//');
    undo(view);
    expect(view.state.doc.toString()).toBe('abc\ndef');
  });

  it.each(LANGUAGE_NAMES)("given %s, when the profile's line comment is compared with the grammar's, then they are the same (the drift guard for the profile table)", (language) => {
    expect(LANGUAGE_PROFILES[language].lineComment).toBe(lineComment(mount(language, '')));
  });

  it.each(LANGUAGE_NAMES)('given %s, when Enter is pressed after the block opener, then the next line is one level deeper', (language) => {
    const text = OPENERS[language];
    const view = mount(language, text, { anchor: text.length });

    insertNewlineAndIndent(view);

    expect(view.state.doc.toString()).toBe(`${text}\n    `);
  });

  it('given Python, when Enter is pressed after a brace, then the next line does not indent as a block would', () => {
    const view = mount('python', 'x = 1 {', { anchor: 7 });

    insertNewlineAndIndent(view);

    expect(view.state.doc.toString()).toBe('x = 1 {\n');
  });

  it.each(LANGUAGE_NAMES)('given %s, when an opening bracket and a quote are typed through the input handlers, then closeBrackets still pairs them', (language) => {
    const view = mount(language, '');
    const type = (text: string) => {
      const { from, to } = view.state.selection.main;
      const insert = () => view.state.update({ changes: { from, to, insert: text }, selection: { anchor: from + text.length }, userEvent: 'input.type' });
      const handled = view.state.facet(EditorView.inputHandler).some((handler) => handler(view, from, to, text, insert));
      if (!handled) view.dispatch(insert());
    };

    type('(');
    expect(view.state.doc.toString()).toBe('()');
    type('"');
    expect(view.state.doc.toString()).toBe('("")');
  });
});
