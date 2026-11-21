import { syntaxTree } from '@codemirror/language';

import type { SyntaxNode } from '../../src';

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
