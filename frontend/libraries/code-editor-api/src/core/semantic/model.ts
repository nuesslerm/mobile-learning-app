import type { EditorError, Language, NotANodeReason, Pos, Range, SyntaxNode } from '../../ports/protocol';
import { classify, errorRanges, familyOf, isDamaged, outline, statementAt } from '../syntax';
import { lineEndOf, lineStartOf } from '../syntax/blocks';
import { frozenLines } from './indent';

/** What a planner reads: the document's text and its full tree (the outline is derived from it). */
export type RunInput = { readonly text: string; readonly root: SyntaxNode; readonly language: Language };

export type Run = {
  /** the sibling nodes the range names, the comments between them included */
  readonly run: readonly SyntaxNode[];
  /** the block or the root they sit in, and the compound or clause whose block that is (null at the root and in a bare block) */
  readonly parent: SyntaxNode;
  readonly owner: SyntaxNode | null;
  /** where the first node stands among `parent.children` */
  readonly index: number;
  /** whole lines: from the first node's line, or its leading comment lines, to after the newline of the last node's line */
  readonly extent: Range;
  /** the whitespace in front of the first node, as the string it is */
  readonly indentPrefix: string;
  /** the extent ends the document and the document has no final newline */
  readonly unterminated: boolean;
};
export type NotANode = Extract<EditorError, { readonly code: 'not-a-node' }>;
export type Located = { readonly ok: true; readonly value: Run } | { readonly ok: false; readonly error: NotANode };

type Hit = { readonly node: SyntaxNode; readonly parent: SyntaxNode; readonly grandparent: SyntaxNode | null };

const isSpace = (char: string | undefined) => char !== undefined && /\s/.test(char);

// a compound's extent runs on to its trailing newline and a simple statement's does not, so a node ends where its text does
function contentEnd(text: string, node: SyntaxNode): Pos {
  let end = node.to;
  while (end > node.from && isSpace(text[end - 1])) end -= 1;
  return end;
}

/** The outermost outline node that starts at `pos`, with the two nodes above it. */
function hitAt(language: Language, root: SyntaxNode, pos: Pos): Hit | null {
  const visit = (node: SyntaxNode, above: SyntaxNode | null): Hit | null => {
    for (const child of node.children ?? []) {
      if (child.from === pos && classify(language, child) !== 'error') return { node: child, parent: node, grandparent: above };
      const found = child.from < pos && pos < child.to ? visit(child, node) : null;
      if (found) return found;
    }
    return null;
  };
  return visit(root, null);
}

// what may follow the last statement of a run on its line: nothing, or comments
function endsLine(rest: string, language: Language): boolean {
  let tail = rest.trim();
  if (language === 'python') return tail === '' || tail.startsWith('#');
  while (tail.startsWith('/*')) {
    const close = tail.indexOf('*/', 2);
    if (close < 0) return false;
    tail = tail.slice(close + 2).trim();
  }
  return tail === '' || tail.startsWith('//');
}

function isCommentLine(rest: string, language: Language): boolean {
  if (language === 'python') return rest.startsWith('#');
  const trimmed = rest.trimEnd();
  return rest.startsWith('//') || (rest.startsWith('/*') && trimmed.endsWith('*/') && trimmed.indexOf('*/') === trimmed.length - 2);
}

/**
 * The whole-line run of sibling statements that `range` names. The range may carry its indentation and its newline. An explicit empty range names nothing; with `caret` (the range is
 * the live selection) an empty one stands for the innermost statement at the caret. Contiguous comment lines right above the first statement, at its indent, belong to the run.
 */
export function locateRun(input: RunInput, range: Range, options: { readonly caret?: boolean } = {}): Located {
  const { text, language } = input;
  const tree = outline(language, input.root);
  const refuse = (reason: NotANodeReason, message: string): Located => ({ ok: false, error: { code: 'not-a-node', message, range, reason } });
  let { from, to } = range;
  if (options.caret && from === to) {
    const statement = statementAt({ language, parsedTo: text.length, root: tree }, from);
    if (!statement) return refuse('no-statement', 'there is no statement at the caret');
    ({ from, to } = statement);
  }
  while (from < to && isSpace(text[from])) from += 1;
  while (to > from && isSpace(text[to - 1])) to -= 1;
  if (from === to) return refuse('no-statement', 'the range holds no text');

  const hit = hitAt(language, tree, from);
  const kind = hit ? classify(language, hit.node) : 'error';
  if (!hit || (kind !== 'simple' && kind !== 'compound' && kind !== 'comment')) return refuse('no-statement', 'the range does not start at a statement');
  const parentKind = classify(language, hit.parent);
  if (parentKind !== 'block' && parentKind !== 'root') return refuse('not-in-block', 'the statement is not in a block: it is a braceless body or an else-if');
  const siblings = hit.parent.children ?? [];
  const index = siblings.indexOf(hit.node);
  const last = siblings.findIndex((sibling, at) => at >= index && contentEnd(text, sibling) === to);
  if (last < 0) return refuse('no-statement', 'the range ends inside a statement');
  const run = siblings.slice(index, last + 1);
  const kinds = run.map((node) => classify(language, node));
  if (kinds.includes('clause') || !kinds.some((one) => one === 'simple' || one === 'compound')) return refuse('no-statement', 'the range holds no whole statement');

  const above = hit.grandparent && ['compound', 'clause'].includes(classify(language, hit.grandparent)) ? hit.grandparent : null;
  const unclosed = familyOf(language) === 'braces' && parentKind === 'block' && text[hit.parent.to - 1] !== '}';
  if (unclosed || isDamaged(hit.parent) || (above !== null && isDamaged(above)) || run.some((node) => errorRanges(node).length > 0)) {
    return refuse('damaged', 'the statement or the block around it has a syntax error');
  }

  const first = run[0] as SyntaxNode;
  const firstLine = lineStartOf(text, first.from);
  const indentPrefix = text.slice(firstLine, first.from);
  if (indentPrefix.trim() !== '' || !endsLine(text.slice(to, lineEndOf(text, to)), language)) return refuse('shares-line', 'another statement is on the same line');

  let start = firstLine;
  if (kinds[0] !== 'comment') {
    const frozen = frozenLines(text, input.root, language);
    while (start > 0) {
      const lineAbove = lineStartOf(text, start - 1);
      const rest = text.slice(lineAbove, start - 1);
      if (frozen.has(lineAbove) || !rest.startsWith(indentPrefix) || !isCommentLine(rest.slice(indentPrefix.length), language)) break;
      start = lineAbove;
    }
  }
  const end = lineEndOf(text, to);
  return { ok: true, value: { run, parent: hit.parent, owner: above, index, extent: { from: start, to: end < text.length ? end + 1 : end }, indentPrefix, unterminated: end >= text.length } };
}
