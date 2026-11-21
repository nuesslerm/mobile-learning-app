import type { Pos, Range, SyntaxNode, TreeValue } from '../../ports/protocol';
import { classify, familyOf, type SyntaxFamily } from './classify';
import { isDamaged, isUnread, pathAt } from './tree';

/** What the functions below read: a tree reply, or anything shaped like one. */
export type TreeInput = Pick<TreeValue, 'language' | 'parsedTo' | 'root'>;

export type EnclosingBlock = {
  /** the statement whose header introduces the block: an `if`, a function, a catch clause; null for the whole document */
  readonly owner: SyntaxNode | null;
  readonly block: SyntaxNode;
  /** what lies between the block's delimiters: after the colon, or inside the braces */
  readonly content: Range;
  /** python: the column the block's statements start at */
  readonly indent?: number;
  /** braces: the closing `}` is there; always true for an indented block */
  readonly closed: boolean;
  /** the block or its header has an error node of its own, or the brace is missing */
  readonly damaged: boolean;
  readonly family: SyntaxFamily;
};

type Located = { readonly block: SyntaxNode; readonly parent: SyntaxNode | null };

const lineStartOf = (text: string, pos: Pos): Pos => (pos === 0 ? 0 : text.lastIndexOf('\n', pos - 1) + 1);
const lineEndOf = (text: string, pos: Pos): Pos => {
  const end = text.indexOf('\n', pos);
  return end < 0 ? text.length : end;
};
const columnOf = (text: string, pos: Pos): number => pos - lineStartOf(text, pos);

function blocksOf(value: TreeInput, root: SyntaxNode): readonly Located[] {
  const found: Located[] = [];
  const visit = (node: SyntaxNode, parent: SyntaxNode | null) => {
    if (classify(value.language, node) === 'block') found.push({ block: node, parent });
    node.children?.forEach((child) => visit(child, node));
  };
  visit(root, null);
  return found;
}

const ownerOf = (value: TreeInput, parent: SyntaxNode | null): SyntaxNode | null => {
  if (!parent) return null;
  const kind = classify(value.language, parent);
  return kind === 'compound' || kind === 'clause' ? parent : null;
};

const isStatement = (value: TreeInput, node: SyntaxNode): boolean => {
  const kind = classify(value.language, node);
  return kind === 'simple' || kind === 'compound';
};

/** The innermost statement at `pos`; with a caret between two statements, the one that starts there. */
export function statementAt(value: TreeInput, pos: Pos): SyntaxNode | null {
  if (!value.root || isUnread(value, pos)) return null;
  let best: { node: SyntaxNode; depth: number } | null = null;
  for (const side of [-1, 1] as const) {
    const path = pathAt(value.root, pos, side);
    let depth = path.length - 1;
    while (depth >= 0 && !isStatement(value, path[depth] as SyntaxNode)) depth -= 1;
    if (depth >= 0 && (!best || depth >= best.depth)) best = { node: path[depth] as SyntaxNode, depth };
  }
  return best?.node ?? null;
}

/** The block a caret at `pos` is typing into, or null where the tree has not been read. The text is needed for an indented block, which has no closing mark: a blank line belongs to the block its column asks for. */
export function enclosingBlock(value: TreeInput, text: string, pos: Pos): EnclosingBlock | null {
  const root = value.root;
  if (!root || isUnread(value, pos)) return null;
  const family = familyOf(value.language);
  const blocks = blocksOf(value, root);
  const chosen = family === 'indent' ? indentedBlockAt(blocks, text, pos) : bracedBlockAt(blocks, text, pos);
  const owner = chosen ? ownerOf(value, chosen.parent) : null;
  const block = chosen?.block ?? root;
  const closed = family === 'indent' || !chosen || text[block.to - 1] === '}';
  const first = block.children?.find((child) => !child.error);
  const delimited = chosen !== null;
  return {
    owner,
    block,
    content: family === 'indent' ? { from: delimited ? block.from + 1 : block.from, to: block.to } : delimited ? { from: block.from + 1, to: closed ? block.to - 1 : block.to } : { from: block.from, to: block.to },
    ...(family === 'indent' ? (first ? { indent: columnOf(text, first.from) } : delimited ? {} : { indent: 0 }) : {}),
    closed,
    damaged: !closed || isDamaged(block) || (owner ? isDamaged(owner) : false),
    family,
  };
}

function bracedBlockAt(blocks: readonly Located[], text: string, pos: Pos): Located | null {
  const inside = blocks.filter(({ block }) => block.from < pos && (pos < block.to || (pos === block.to && text[block.to - 1] !== '}')));
  return inside.reduce<Located | null>((best, one) => (!best || one.block.from > best.block.from ? one : best), null);
}

function indentedBlockAt(blocks: readonly Located[], text: string, pos: Pos): Located | null {
  const start = lineStartOf(text, pos);
  const blank = text.slice(start, lineEndOf(text, pos)).trim() === '';
  const innermostFirst = [...blocks].sort((a, b) => b.block.from - a.block.from);
  if (!blank) return innermostFirst.find(({ block }) => block.from < pos && (pos < block.to || (pos === block.to && text[pos - 1] !== '\n'))) ?? null;
  const column = pos - start;
  const reachable = innermostFirst.filter(({ block }) => block.from < start && (block.to >= start || text.slice(block.to, start).trim() === ''));
  return (
    reachable.find(({ block }) => {
      const first = block.children?.find((child) => !child.error);
      return first !== undefined && columnOf(text, first.from) <= column;
    }) ?? null
  );
}
