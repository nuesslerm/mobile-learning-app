import { classify, errorRanges, isDamaged, lineEndOf, lineStartOf, outline, STATEMENT_BLOCK, type Destination, type Language, type Pos, type Range, type Revision, type SyntaxNode, type TreeValue } from '@lib/code-editor-api';

import { kindOfNode, type BlockKind } from './blockKinds';

/** Past this many rows the rest is one text row: a drag cannot cross more than a screen of rows and the page cannot scroll while it runs. */
export const MAX_ROWS = 30;

export type BlockRow = {
  /** the path of the statement: `3` at the root, `3.0.1` for the second statement of the first block of the fourth one; a clause line is `3.c1`, the unread rest `unread` */
  readonly id: string;
  readonly index: number;
  readonly depth: number;
  readonly kind: BlockKind;
  /** the statement's first line, trimmed */
  readonly label: string;
  /** the statement as the engine takes it: from its first character to its last, trailing whitespace left out */
  readonly range: Range;
  /** the statement can be dragged: the engine would take it as the node of a move (nothing in it is damaged, nothing else is on its line) and its block would not be left empty */
  readonly movable: boolean;
  /** the index of the first row after this statement and everything under it */
  readonly subtreeEnd: number;
};

export type BlockSlot = {
  readonly id: string;
  /** the row boundary the drop line is drawn at: 0 above the first row, `rows.length` below the last */
  readonly gap: number;
  readonly depth: number;
  /** the canonical destination: after the statement before, else into the owner at the start of its block, else before the first statement of the document */
  readonly destination: Destination;
  /** the engine would take the destination: its reference statement can be found as a node and its owner is whole */
  readonly valid: boolean;
  /** the row id of the compound whose block this is; null at the root */
  readonly owner: string | null;
  readonly block: number;
  /** among the statements of the block: the slot before the first one is 0 */
  readonly position: number;
};

export type BlockModel = {
  readonly text: string;
  readonly language: Language;
  readonly instance: string;
  readonly revision: Revision;
  readonly complete: boolean;
  /** where the text the parse did not reach starts: it is shown as one text row */
  readonly unreadFrom: Pos | null;
  readonly rows: readonly BlockRow[];
  readonly slots: readonly BlockSlot[];
};

/** Where a statement stands: its owner's id (null at the root), the block of the owner it is in, and its place in that block. */
export function placeOf(id: string): { readonly owner: string | null; readonly block: number; readonly position: number } {
  const parts = id.split('.');
  const position = Number(parts.at(-1));
  if (parts.length === 1) return { owner: null, block: 0, position };
  return { owner: parts.slice(0, -2).join('.'), block: Number(parts.at(-2)), position };
}

type Row = { -readonly [K in keyof BlockRow]: BlockRow[K] };
type Item = { readonly row: Row; readonly locatable: boolean };
type Owner = { readonly id: string; readonly range: Range; readonly sound: boolean; readonly open: boolean };

const isSpace = (char: string | undefined) => char !== undefined && /\s/.test(char);

function contentEnd(text: string, from: Pos, to: Pos): Pos {
  let end = to;
  while (end > from && isSpace(text[end - 1])) end -= 1;
  return end;
}

/** The first and last non-blank characters of the span, or null for a blank one. */
function trimmed(text: string, from: Pos, to: Pos): Range | null {
  let start = from;
  while (start < to && isSpace(text[start])) start += 1;
  return start < to ? { from: start, to: contentEnd(text, start, to) } : null;
}

const lineAt = (text: string, from: Pos): string => text.slice(from, lineEndOf(text, from)).trim();

// what may follow a statement on its line: nothing, or a comment
const endsLine = (rest: string): boolean => {
  const tail = rest.trim();
  return tail === '' || tail.startsWith('#');
};

const sharesLine = (text: string, node: SyntaxNode, end: Pos): boolean => text.slice(lineStartOf(text, node.from), node.from).trim() !== '' || !endsLine(text.slice(end, lineEndOf(text, end)));

const bodiesOf = (node: SyntaxNode): SyntaxNode[] => (node.children ?? []).filter((child) => child.type === STATEMENT_BLOCK.python);

const emptyModel = (input: { text: string; tree: TreeValue }): BlockModel => ({
  text: input.text,
  language: input.tree.language,
  instance: input.tree.instance,
  revision: input.tree.revision,
  complete: input.tree.complete,
  unreadFrom: null,
  rows: [],
  slots: [],
});

/**
 * The rows and drop slots of a Python document, from the text and a tree reply: statements only, a compound as a header row with its blocks one depth deeper, an else or except line as a clause
 * row. A row is `movable` and a slot `valid` exactly where the engine's node steps would take them (`locateRun` and `resolveDestination`: the differential test in dropPlan.engine.test.ts holds
 * this); a statement that cannot be read as one (an error inside it, another statement on its line) or that lies past what the parse reached stays a plain text row.
 */
export function buildBlockModel(input: { readonly text: string; readonly tree: TreeValue }): BlockModel {
  const { text, tree } = input;
  if (tree.language !== 'python') return emptyModel(input);

  const rows: Row[] = [];
  const slots: BlockSlot[] = [];
  const limit = tree.complete ? Number.POSITIVE_INFINITY : tree.parsedTo;
  const cut: { at: Pos | null } = { at: null };

  const pushText = (id: string, depth: number, range: Range, label = lineAt(text, range.from)): Row => {
    const row: Row = { id, index: rows.length, depth, kind: 'text', label, range, movable: false, subtreeEnd: rows.length + 1 };
    rows.push(row);
    return row;
  };

  const emit = (node: SyntaxNode, id: string, depth: number, intact: boolean): Row => {
    const range = { from: node.from, to: contentEnd(text, node.from, node.to) };
    const compound = classify('python', node) === 'compound';
    const bodies = compound ? bodiesOf(node) : [];
    const clean = errorRanges(node).length === 0;
    const alone = !sharesLine(text, node, range.to);
    const row: Row = { id, index: rows.length, depth, kind: 'statement', label: lineAt(text, node.from), range, movable: intact && clean && alone, subtreeEnd: rows.length + 1 };
    rows.push(row);
    if (bodies.length === 0) {
      row.kind = !clean || !alone ? 'text' : compound ? 'statement' : kindOfNode(node.type);
      if (row.kind === 'text') row.movable = false;
      return row;
    }
    row.kind = kindOfNode(node.type);
    const sound = !isDamaged(node);
    const owner: Owner = { id, range, sound, open: sound && bodies.every((body) => !isDamaged(body)) };
    bodies.forEach((body, index) => {
      if (index > 0) {
        const line = trimmed(text, lineStartOf(text, body.from), lineEndOf(text, body.from));
        const clause = pushText(`${id}.c${index}`, depth, line ?? { from: body.from, to: body.from });
        clause.kind = 'clause';
      }
      walk(body, owner, index, depth + 1, rows.length, false);
    });
    row.subtreeEnd = rows.length;
    return row;
  };

  const walk = (container: SyntaxNode, owner: Owner | null, blockIndex: number, depth: number, startGap: number, isRoot: boolean): void => {
    const intact = !isDamaged(container) && (owner === null || owner.sound);
    const items: Item[] = [];
    for (const child of container.children ?? []) {
      if (isRoot && child.to >= limit) {
        cut.at = child.from;
        break;
      }
      const kind = classify('python', child);
      const id = owner ? `${owner.id}.${blockIndex}.${items.length}` : `${items.length}`;
      if (kind === 'error') {
        const spot = trimmed(text, child.from, child.to);
        if (spot) items.push({ row: pushText(id, depth, spot), locatable: false });
      } else if (kind === 'simple' || kind === 'compound') {
        const row = emit(child, id, depth, intact);
        items.push({ row, locatable: row.movable });
      }
    }
    // taking the only statement out of a Python block leaves its header without a body, which the engine refuses: such a statement can be a reference but not be moved
    const only = owner !== null && items.length === 1 ? items[0] : undefined;
    if (only) only.row.movable = false;

    const add = (position: number, destination: Destination, gap: number, valid: boolean) =>
      slots.push({ id: `${owner?.id ?? 'root'}:${blockIndex}:${position}`, gap, depth, destination, valid, owner: owner?.id ?? null, block: blockIndex, position });
    const first = items[0];
    const into: Destination | null = owner ? { into: owner.range, at: 'start', block: blockIndex } : null;
    if (!first) {
      if (into) add(0, into, startGap, false);
      return;
    }
    add(0, into ?? { before: first.row.range }, into ? startGap : first.row.index, first.locatable && (owner === null || owner.open));
    items.forEach(({ row, locatable }, index) => add(index + 1, { after: row.range }, row.subtreeEnd, locatable && !(isRoot && !tree.complete && index === items.length - 1)));
  };

  if (tree.root) walk(outline('python', tree.root), null, 0, 0, 0, true);

  let unreadFrom: Pos | null = null;
  if (!tree.complete) {
    const from = cut.at ?? tree.parsedTo;
    const rest = trimmed(text, from, text.length);
    if (rest) {
      unreadFrom = from;
      pushText('unread', 0, rest);
    }
  }

  const capped = rows.length > MAX_ROWS;
  const hidden = rows.length - MAX_ROWS;
  const shown: BlockRow[] = capped ? rows.slice(0, MAX_ROWS).map((row) => (row.subtreeEnd > MAX_ROWS ? { ...row, subtreeEnd: MAX_ROWS } : row)) : rows;
  if (capped) {
    const from = (rows[MAX_ROWS] as Row).range.from;
    shown.push({ id: 'more', index: MAX_ROWS, depth: 0, kind: 'text', label: `… ${hidden} more rows`, range: trimmed(text, from, text.length) ?? { from, to: from }, movable: false, subtreeEnd: MAX_ROWS + 1 });
  }
  const kept = capped ? slots.filter((slot) => slot.gap <= MAX_ROWS) : slots;
  kept.sort((a, b) => a.gap - b.gap || a.depth - b.depth);

  return { text, language: tree.language, instance: tree.instance, revision: tree.revision, complete: tree.complete, unreadFrom, rows: shown, slots: kept };
}
