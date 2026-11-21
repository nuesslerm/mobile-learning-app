import type { Pos, Range, SyntaxNode, TreeValue } from '../../ports/protocol';

/** 1: the node starting at the position wins; -1: the node ending there wins */
export type Side = -1 | 1;

const holds = (node: SyntaxNode, pos: Pos, side: Side): boolean => (side === 1 ? node.from <= pos && pos < node.to : node.from < pos && pos <= node.to);

const overlaps = (node: SyntaxNode, range: Range): boolean => {
  if (range.from === range.to) return node.from <= range.from && range.from <= node.to;
  return node.from === node.to ? range.from <= node.from && node.from <= range.to : node.from < range.to && node.to > range.from;
};

/** The nodes from the root down to the innermost one at `pos`; a node that only touches the position counts, `side` picks between the two that do. */
export function pathAt(root: SyntaxNode | null, pos: Pos, side: Side = 1): readonly SyntaxNode[] {
  if (!root || pos < root.from || pos > root.to) return [];
  const path = [root];
  let node = root;
  for (;;) {
    const next = node.children?.find((child) => holds(child, pos, side));
    if (!next) return path;
    path.push(next);
    node = next;
  }
}

export function nodeAt(root: SyntaxNode | null, pos: Pos, side: Side = 1): SyntaxNode | null {
  return pathAt(root, pos, side).at(-1) ?? null;
}

/** In document order, parents before children; a point range gives the nodes touching it, `within` only the nodes inside the range. */
export function nodesInRange(root: SyntaxNode | null, range: Range, options: { readonly within?: boolean } = {}): readonly SyntaxNode[] {
  const found: SyntaxNode[] = [];
  const visit = (node: SyntaxNode) => {
    if (!overlaps(node, range)) return;
    if (!options.within || (node.from >= range.from && node.to <= range.to)) found.push(node);
    node.children?.forEach(visit);
  };
  if (root) visit(root);
  return found;
}

/** The extents of the outermost error nodes, in document order. */
export function errorRanges(root: SyntaxNode | null, range?: Range): readonly Range[] {
  const found: Range[] = [];
  const visit = (node: SyntaxNode) => {
    if (range && !overlaps(node, range)) return;
    if (node.error) found.push({ from: node.from, to: node.to });
    else node.children?.forEach(visit);
  };
  if (root) visit(root);
  return found;
}

/** The node has an error node as a child: its own shape could not be read. */
export const isDamaged = (node: SyntaxNode): boolean => node.children?.some((child) => child.error === true) ?? false;

/** Past what the parser reached, the tree says nothing: callers treat the text there as plain text. */
export const isUnread = (value: Pick<TreeValue, 'parsedTo'>, pos: Pos): boolean => pos > value.parsedTo;

/** The tree cut to what touches `range`: the root always stays, a child stays when it overlaps the range. */
export function cutToRange(root: SyntaxNode, range: Range): SyntaxNode {
  const cut = (node: SyntaxNode): SyntaxNode => {
    const kept = node.children?.filter((child) => overlaps(child, range)).map(cut) ?? [];
    return { type: node.type, from: node.from, to: node.to, ...(node.error ? { error: true as const } : {}), ...(kept.length > 0 ? { children: kept } : {}) };
  };
  return cut(root);
}

/** The tree as a parse that reached only `limit` would have it: nothing starts at or after the limit, nothing ends past it. */
export function clipTo(root: SyntaxNode, limit: Pos): SyntaxNode {
  const clip = (node: SyntaxNode): SyntaxNode => {
    const kept = node.children?.filter((child) => child.from < limit).map(clip) ?? [];
    return { type: node.type, from: node.from, to: Math.min(node.to, limit), ...(node.error ? { error: true as const } : {}), ...(kept.length > 0 ? { children: kept } : {}) };
  };
  return clip(root);
}
