import type { Language, SyntaxNode, TreeValue } from '../../../src';

export const node = (type: string, from: number, to: number, children?: readonly SyntaxNode[]): SyntaxNode => ({ type, from, to, ...(children ? { children } : {}) });
export const error = (at: number): SyntaxNode => ({ type: '⚠', from: at, to: at, error: true });

/** The offset where `snippet` starts, counting from the `nth` occurrence: trees are built from the text, never from hand-counted offsets. */
export const at = (text: string, snippet: string, nth = 0): number => {
  let from = -1;
  for (let i = 0; i <= nth; i += 1) {
    from = text.indexOf(snippet, from + 1);
    if (from < 0) throw new Error(`no occurrence ${nth} of ${JSON.stringify(snippet)}`);
  }
  return from;
};
export const endOf = (text: string, snippet: string, nth = 0): number => at(text, snippet, nth) + snippet.length;
export const span = (type: string, text: string, snippet: string, children?: readonly SyntaxNode[], nth = 0): SyntaxNode => node(type, at(text, snippet, nth), endOf(text, snippet, nth), children);

export const reply = (language: Language, root: SyntaxNode | null, length: number, parsedTo = length): TreeValue => ({ instance: 'test', revision: 0, language, length, parsedTo, complete: parsedTo >= length, detail: 'full', root });
