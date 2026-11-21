import type { Change, EditorError, Language, Pos, Range, SyntaxNode } from '../../ports/protocol';
import { mapPos, mapRange } from '../changes';
import { classify, errorRanges, outline } from '../syntax';

/** The type nesting of a tree. `is` is a Lezer type name, or a class wildcard (`compound`, `block`, `simple`, ...) in an expectation; `children` left out means any. */
export type Shape = { readonly is: string; readonly children?: readonly Shape[] };

export type Expectation = {
  /** ranges of the base document the step removes: the nodes wholly inside them are left out of the comparison */
  readonly removed?: readonly Range[];
  /** where the nodes the step writes must sit: the root, or the block that starts at this position of the base document; and what they must look like, comments apart */
  readonly writes?: { readonly parent: 'root' | Pos; readonly shapes?: readonly Shape[] };
};

export type VerifyInput = {
  readonly language: Language;
  /** the full trees of the document before and after the changes */
  readonly base: SyntaxNode;
  readonly result: SyntaxNode;
  readonly changes: readonly Change[];
  /** per step, the text it writes in the result document (`Plan.written`), and what it promised, null for a step with no promise */
  readonly written: readonly (Range | null)[];
  readonly expectations: readonly (Expectation | null)[];
};
export type WouldNotParse = Extract<EditorError, { readonly code: 'would-not-parse' }>;
export type Verified = { readonly ok: true } | { readonly ok: false; readonly error: WouldNotParse };

const dropped = (language: Language, node: SyntaxNode) => {
  const kind = classify(language, node);
  return kind === 'comment' || kind === 'error';
};
const shapeChildren = (language: Language, node: SyntaxNode): readonly SyntaxNode[] => (node.children ?? []).filter((child) => !dropped(language, child));

/** The types and the nesting of an outline: comments and error nodes are not part of it. */
export function shapeOf(language: Language, tree: SyntaxNode): Shape {
  return { is: tree.type, children: shapeChildren(language, tree).map((child) => shapeOf(language, child)) };
}

const inside = (node: SyntaxNode, range: Range) => node.from >= range.from && node.to <= range.to;

/** The outline without the nodes that lie wholly inside one of `ranges`; the root stays. */
export function prune(tree: SyntaxNode, ranges: readonly Range[]): SyntaxNode {
  const cut = (node: SyntaxNode): SyntaxNode => {
    const kept = (node.children ?? []).filter((child) => !ranges.some((range) => inside(child, range))).map(cut);
    return { type: node.type, from: node.from, to: node.to, ...(node.error ? { error: true as const } : {}), ...(kept.length > 0 ? { children: kept } : {}) };
  };
  return cut(tree);
}

/** The error ranges of `result` that no error of `base`, carried through `changes`, overlaps: what the edit broke. They are in the result document. */
export function newErrors(base: SyntaxNode, result: SyntaxNode, changes: readonly Change[]): Range[] {
  const before = errorRanges(base).map((range) => mapRange(range, changes));
  return errorRanges(result).filter((range) => !before.some((old) => old.from <= range.to && range.from <= old.to));
}

// the first node of `node` that is not what `want` says, or `node` itself when it has fewer children
function mismatch(language: Language, node: SyntaxNode, want: Shape): SyntaxNode | null {
  if (node.type !== want.is && classify(language, node) !== want.is) return node;
  if (!want.children) return null;
  const children = shapeChildren(language, node);
  for (const [index, child] of children.entries()) {
    const expected = want.children[index];
    const wrong = expected ? mismatch(language, child, expected) : child;
    if (wrong) return wrong;
  }
  return children.length < want.children.length ? node : null;
}

/** The outermost nodes wholly inside `range`, with the node each hangs from. */
function outermostIn(tree: SyntaxNode, range: Range): { readonly node: SyntaxNode; readonly parent: SyntaxNode }[] {
  const found: { node: SyntaxNode; parent: SyntaxNode }[] = [];
  const visit = (node: SyntaxNode) => {
    for (const child of node.children ?? []) {
      if (inside(child, range)) found.push({ node: child, parent: node });
      else if (child.to >= range.from && child.from <= range.to) visit(child);
    }
  };
  visit(tree);
  return found;
}

const refuse = (range: Range, message: string): Verified => ({ ok: false, error: { code: 'would-not-parse', message, errors: [{ from: range.from, to: range.to }], reason: 'structure' } });

/**
 * Checks that an edit that parses also keeps the structure its steps promised. Outside the ranges they declare (removed in the base, written in the result) both outlines have the same
 * shape; and what each step writes is made of statements and comments, sits in the block it was meant for, and has the shape it was meant to have.
 */
export function verifyStructure({ language, base, result, changes, written, expectations }: VerifyInput): Verified {
  const after = outline(language, result);
  const removed = expectations.flatMap((expectation) => expectation?.removed ?? []);
  const outside = mismatch(language, prune(after, written.flatMap((range) => (range ? [range] : []))), shapeOf(language, prune(outline(language, base), removed)));
  if (outside) return refuse(outside, 'the edit changes the structure of the code outside the lines it was meant to touch');

  for (const [step, range] of written.entries()) {
    if (!range) continue;
    const writes = expectations[step]?.writes;
    if (!writes) return refuse(range, 'the edit writes text that no step vouched for');
    const parentFrom = writes.parent === 'root' ? null : mapPos(writes.parent, changes, 1);
    const nodes = outermostIn(after, range);
    for (const { node, parent } of nodes) {
      const kind = classify(language, node);
      if (kind !== 'simple' && kind !== 'compound' && kind !== 'comment') return refuse(node, 'the written text is not made of statements: it joins a clause or a block of the code around it');
      const home = parentFrom === null ? classify(language, parent) === 'root' : classify(language, parent) === 'block' && parent.from === parentFrom;
      if (!home) return refuse(node, 'the written text landed in another block than the one it was meant for');
    }
    const statements = nodes.filter(({ node }) => classify(language, node) !== 'comment');
    if (!writes.shapes) continue;
    if (statements.length !== writes.shapes.length) return refuse(range, 'the written text is not made of the statements the step meant to write');
    for (const [index, { node }] of statements.entries()) {
      const wrong = mismatch(language, node, writes.shapes[index] as Shape);
      if (wrong) return refuse(wrong, 'the written statement does not have the shape the step meant it to have');
    }
  }
  return { ok: true };
}
