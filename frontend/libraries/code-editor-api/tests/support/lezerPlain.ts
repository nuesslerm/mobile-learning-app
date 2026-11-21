import { cppLanguage } from '@codemirror/lang-cpp';
import { javascriptLanguage } from '@codemirror/lang-javascript';
import { pythonLanguage } from '@codemirror/lang-python';
import { ensureSyntaxTree, syntaxTree, type LRLanguage } from '@codemirror/language';
import { EditorState } from '@codemirror/state';

import { cutToRange, outline, type Language, type SyntaxNode, type TreeSnapshot } from '../../src';

export const GRAMMARS: Record<Language, LRLanguage> = { python: pythonLanguage, javascript: javascriptLanguage, cpp: cppLanguage };

export function stateOf(language: Language, doc: string) {
  return EditorState.create({ doc, extensions: [GRAMMARS[language]] });
}

type Tree = ReturnType<typeof syntaxTree>;
type Draft = { type: string; from: number; to: number; error?: true; children: Draft[] };

/** A Lezer tree as plain nodes, for tests only: the named nodes and the error nodes, the shape the page answers `tree` with. */
export function toSyntaxNode(tree: Tree): SyntaxNode {
  const top: Draft = { type: tree.type.name, from: 0, to: tree.length, children: [] };
  const stack: Draft[] = [top];
  const kept: boolean[] = [];
  tree.iterate({
    enter(node) {
      if (node.type.isTop) {
        kept.push(false);
        return;
      }
      const keep = node.type.isError || /^[A-Z]/.test(node.name);
      kept.push(keep);
      if (!keep) return;
      const draft: Draft = { type: node.name, from: node.from, to: node.to, ...(node.type.isError ? { error: true as const } : {}), children: [] };
      (stack.at(-1) as Draft).children.push(draft);
      stack.push(draft);
    },
    leave() {
      if (kept.pop()) stack.pop();
    },
  });
  const plain = (draft: Draft): SyntaxNode => ({
    type: draft.type,
    from: draft.from,
    to: draft.to,
    ...(draft.error ? { error: true as const } : {}),
    ...(draft.children.length > 0 ? { children: draft.children.map(plain) } : {}),
  });
  return plain(top);
}

/** The tree a page would answer with once the parse is complete. */
export function realTree(language: Language, doc: string): SyntaxNode {
  const tree = ensureSyntaxTree(stateOf(language, doc), doc.length, 10_000);
  if (!tree) throw new Error('the parse did not finish');
  return toSyntaxNode(tree);
}

/** A snapshot over a real Lezer parse, for a fake editor's `tree` option: its trees are the ones the page answers with. */
export function lezerSnapshot(text: string, language: Language): TreeSnapshot {
  const state = stateOf(language, text);
  let best = syntaxTree(state);
  return {
    language,
    length: text.length,
    work(upto, ms) {
      const done = ensureSyntaxTree(state, upto, ms);
      if (done) best = done;
      return done !== null;
    },
    parsedTo: () => best.length,
    read(range, detail) {
      const full = toSyntaxNode(best);
      return cutToRange(detail === 'outline' ? outline(language, full) : full, range);
    },
  };
}
