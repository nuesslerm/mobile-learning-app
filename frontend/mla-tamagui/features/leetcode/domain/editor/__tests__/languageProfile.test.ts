import { insertNewlineAndIndent } from '@codemirror/commands';
import { cpp } from '@codemirror/lang-cpp';
import { javascript } from '@codemirror/lang-javascript';
import { python } from '@codemirror/lang-python';
import { indentUnit, type LanguageSupport } from '@codemirror/language';
import { EditorState } from '@codemirror/state';
import type { Language } from '@lib/code-editor-api';
import { describe, expect, it } from 'vitest';

import { smartNewline, toggleComment } from '../editing';
import { LANGUAGE_PROFILES, profileOf, toEditorLanguage } from '../languageProfile';

const caretOf = (marked: string) => ({ doc: marked.replace('|', ''), at: marked.indexOf('|') });

describe('toEditorLanguage', () => {
  it.each([
    ['python', 'python'],
    ['python3', 'python'],
    ['javascript', 'javascript'],
    ['cpp', 'cpp'],
    ['java', null],
    ['', null],
  ])('given the screen language %j, when it is mapped, then the editor language is %j', (id, expected) => {
    expect(toEditorLanguage(id)).toBe(expected);
  });
});

describe('toggleComment under each language', () => {
  const sel = (end: number) => ({ start: 0, end });

  it('given two lines and the JavaScript profile, when Comment runs twice, then both get "// " and then lose it', () => {
    const commented = toggleComment('a\nb', sel(3), profileOf('javascript'));
    expect(commented.text).toBe('// a\n// b');
    expect(toggleComment(commented.text, { start: 0, end: commented.text.length }, profileOf('javascript')).text).toBe('a\nb');
  });

  it('given "//x" with no space, when Comment runs under C++, then only the marker goes', () => {
    expect(toggleComment('//x', sel(3), profileOf('cpp')).text).toBe('x');
  });

  it('given a Python comment under the JavaScript profile, when Comment runs, then it is commented again (a # is not a JavaScript comment)', () => {
    expect(toggleComment('# x', sel(3), profileOf('javascript')).text).toBe('// # x');
  });

  it('given a "//" line under the Python profile, when Comment runs, then a # goes in front of it', () => {
    expect(toggleComment('// x', sel(4), profileOf('python')).text).toBe('# // x');
  });

  it('given an indented line and a mix of blank lines, when Comment runs under JavaScript, then the marker goes after the indent and blanks stay blank', () => {
    expect(toggleComment('    a\n\n    b', sel(11), profileOf('javascript')).text).toBe('    // a\n\n    // b');
  });
});

describe('smartNewline under each language', () => {
  const run = (marked: string, language: Language) => {
    const { doc, at } = caretOf(marked);
    const result = smartNewline(doc, { start: at, end: at }, profileOf(language));
    return `${result.text.slice(0, result.selection.start)}|${result.text.slice(result.selection.start)}`;
  };

  it.each([
    ['a JavaScript line that ends in "{"', 'function f() {|', 'function f() {\n    |'],
    ['a JavaScript line that ends in "("', 'foo(|', 'foo(\n    |'],
    ['a JavaScript line that ends in "["', 'const a = [|', 'const a = [\n    |'],
    ['a JavaScript line that ends in "=>"', 'const g = () =>|', 'const g = () =>\n    |'],
    ['a braceless if', 'if (x)|', 'if (x)\n    |'],
    ['a braceless else if', '} else if (x)|', '} else if (x)\n    |'],
    ['a case label, which does not open a block in the grammar\'s Enter', 'case 1:|', 'case 1:\n|'],
    ['a plain statement', '    x = 1;|', '    x = 1;\n    |'],
    ['the caret between braces', '    {|}', '    {\n        |\n    }'],
    ['the caret between parentheses', '(|)', '(\n    |\n)'],
  ])('given %s, when Newline runs under JavaScript, then the next line is placed as the grammar places it', (_name, before, after) => {
    expect(run(before, 'javascript')).toBe(after);
  });

  it('given a C++ line that ends in "[", when Newline runs, then it does not indent (the C++ grammar has no such opener)', () => {
    expect(run('int a[|', 'cpp')).toBe('int a[\n|');
  });

  // pinned, known divergence: the grammar's Enter also puts the closer of a C++ subscript on its own line
  it('given the caret between the brackets of a C++ subscript, when Newline runs, then it is a plain line break (pinned)', () => {
    expect(run('    int a[|]', 'cpp')).toBe('    int a[\n    |]');
  });

  it('given a Python line that ends in a colon or in a bracket, when Newline runs, then only the colon opens a block (pinned: the grammar\'s Enter also indents after a bracket)', () => {
    expect(run('if x:|', 'python')).toBe('if x:\n    |');
    expect(run('foo(|', 'python')).toBe('foo(\n|');
  });

  it('given a selection, when Newline runs under JavaScript, then the selection is replaced by the line break', () => {
    const result = smartNewline('abXYcd', { start: 2, end: 4 }, profileOf('javascript'));
    expect(result).toEqual({ text: 'ab\ncd', selection: { start: 3, end: 3 } });
  });

  // pinned, known quirk: the line decides as a whole, so a comment or a string that ends in an opener counts
  it('given a comment that ends in "{", when Newline runs under JavaScript, then the next line is indented (pinned, known quirk)', () => {
    expect(run('x = 1; // see {|', 'javascript')).toBe('x = 1; // see {\n    |');
  });
});

// The grammar's own Enter is the reference: a block opener or a closer the profile does not know shows up here, in a body, where
// the grammar's indentation and the line's own indentation are the same thing.
describe('smartNewline against CodeMirror\'s own Enter', () => {
  const grammars: Record<Language, () => LanguageSupport> = { python, javascript, cpp };

  function codeMirrorEnter(language: Language, marked: string) {
    const { doc, at } = caretOf(marked);
    let state = EditorState.create({ doc, selection: { anchor: at }, extensions: [grammars[language](), indentUnit.of('    ')] });
    insertNewlineAndIndent({ state, dispatch: (tr) => (state = tr.state) });
    return { text: state.doc.toString(), caret: state.selection.main.head };
  }

  function ours(language: Language, marked: string) {
    const { doc, at } = caretOf(marked);
    const result = smartNewline(doc, { start: at, end: at }, LANGUAGE_PROFILES[language]);
    return { text: result.text, caret: result.selection.start };
  }

  const BODY = (line: string) => `function f() {\n    ${line}\n}`;
  const CASES: Record<Language, string[]> = {
    javascript: [
      BODY('x = 1;|'),
      BODY('foo(|'),
      BODY('const a = [|'),
      BODY('const g = () =>|'),
      BODY('if (x) {|'),
      BODY('if (x)|'),
      BODY('while (x)|'),
      BODY('for (let i = 0; i < n; i++)|'),
      BODY('else {|'),
      BODY('return x;|'),
      BODY('{|}'),
      BODY('foo(|)'),
      BODY('const a = [|]'),
      '{|}',
      '(|)',
      '[|]',
    ],
    cpp: [
      BODY('x = 1;|'),
      BODY('foo(|'),
      BODY('if (x) {|'),
      BODY('if (x)|'),
      BODY('while (x)|'),
      BODY('for (int i = 0; i < n; i++)|'),
      BODY('return x;|'),
      BODY('{|}'),
      BODY('foo(|)'),
      '{|}',
    ],
    python: [
      'def f():\n    x = 1|',
      'def f():\n    if x:|',
      'def f():\n    return 1|',
      'class A:\n    def f(self):|',
      'while True:\n    x = 1|',
    ],
  };

  for (const language of Object.keys(CASES) as Language[]) {
    it.each(CASES[language])(`given ${language} %j, when Newline runs, then it matches the grammar's Enter`, (marked) => {
      expect(ours(language, marked)).toEqual(codeMirrorEnter(language, marked));
    });
  }
});
