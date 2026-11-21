import type { syntaxTree } from '@codemirror/language';
import { cutToRange, outline, type Language, type Range, type SyntaxNode, type TreeDetail } from '@lib/code-editor-api';

type Tree = ReturnType<typeof syntaxTree>;
type Draft = { type: string; from: number; to: number; error?: true; children: Draft[] };

const plain = (draft: Draft): SyntaxNode => ({
  type: draft.type,
  from: draft.from,
  to: draft.to,
  ...(draft.error ? { error: true as const } : {}),
  ...(draft.children.length > 0 ? { children: draft.children.map(plain) } : {}),
});

/**
 * A Lezer tree as the plain nodes the typed API answers with: the named nodes (a capital letter) and the error nodes, cut to what touches
 * `range`, and for `outline` only what editing is about (statements and blocks). The tree's own length is how far it was parsed.
 */
export function readSyntaxNode(tree: Tree, language: Language, range: Range, detail: TreeDetail): SyntaxNode {
  const top: Draft = { type: tree.type.name, from: 0, to: tree.length, children: [] };
  const stack: Draft[] = [top];
  const kept: boolean[] = [];
  tree.iterate({
    from: range.from,
    to: range.to,
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
  const root = plain(top);
  return cutToRange(detail === 'outline' ? outline(language, root) : root, range);
}
