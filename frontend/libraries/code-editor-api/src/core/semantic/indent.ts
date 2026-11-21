import type { Language, Pos, Range, SyntaxNode } from '../../ports/protocol';
import { familyOf, MULTILINE_LITERALS } from '../syntax/classify';

export const DEFAULT_INDENT_UNIT = '    ';

/** One line of the document without its newline; `from` is where it starts. */
export type Line = { readonly from: Pos; readonly text: string };

/** The lines of a whole-line range (it ends after a newline, or at the end of the text). */
export function linesOf(text: string, range: Range): Line[] {
  const lines: Line[] = [];
  let from = range.from;
  while (from < range.to) {
    const newline = text.indexOf('\n', from);
    const end = newline < 0 || newline >= range.to ? range.to : newline;
    lines.push({ from, text: text.slice(from, end) });
    from = end + 1;
  }
  return lines;
}

/** The starts of the lines that begin inside a multi-line literal: their text is not code and keeps its columns. Needs the full tree, which is the only one that holds the literals. */
export function frozenLines(text: string, root: SyntaxNode | null, language: Language): ReadonlySet<Pos> {
  const literals = new Set<string>(MULTILINE_LITERALS[language]);
  const frozen = new Set<Pos>();
  const visit = (node: SyntaxNode) => {
    if (!literals.has(node.type)) {
      node.children?.forEach(visit);
      return;
    }
    for (let at = text.indexOf('\n', node.from); at >= 0 && at + 1 < node.to; at = text.indexOf('\n', at + 1)) frozen.add(at + 1);
  };
  if (root) visit(root);
  return frozen;
}

/**
 * Swaps the indentation prefix `fromPrefix` for `toPrefix` on each line, as the strings they are: tabs stay tabs and nothing is converted. A blank line, a line that does not
 * start with the prefix (a continuation) and a frozen line are left as they are.
 */
export function shiftPrefix(lines: readonly Line[], fromPrefix: string, toPrefix: string, frozen: ReadonlySet<Pos>): string[] {
  return lines.map(({ from, text }) => (frozen.has(from) || text.trim() === '' || !text.startsWith(fromPrefix) ? text : toPrefix + text.slice(fromPrefix.length)));
}

/** Where a position of `lines` stands in `shifted.join('\n')`, the same lines after `shiftPrefix` swapped `fromPrefix` for `toPrefix` in those that changed. */
export function carryPos(lines: readonly Line[], shifted: readonly string[], fromPrefix: string, toPrefix: string): (pos: Pos) => number {
  return (pos) => {
    let before = 0;
    for (const [index, line] of lines.entries()) {
      const now = shifted[index] as string;
      if (pos <= line.from + line.text.length) {
        const col = pos - line.from;
        return before + (now === line.text ? col : col >= fromPrefix.length ? col - fromPrefix.length + toPrefix.length : Math.min(col, toPrefix.length));
      }
      before += now.length + 1;
    }
    return Math.max(0, before - 1);
  };
}

/**
 * How many of `blocks` surround each non-blank line of `extent`, counted at the line's first character (the key is the line's start). A line that opens or closes a braced block,
 * and a python `else` or `elif` line between two bodies, is not inside the block and has its owner's depth.
 */
export function depthMap(language: Language, text: string, blocks: readonly SyntaxNode[], extent: Range): ReadonlyMap<Pos, number> {
  const braces = familyOf(language) === 'braces';
  const innerEnd = (block: SyntaxNode) => (braces && text[block.to - 1] === '}' ? block.to - 1 : block.to);
  const depths = new Map<Pos, number>();
  for (const line of linesOf(text, extent)) {
    if (line.text.trim() === '') continue;
    const first = line.from + line.text.length - line.text.trimStart().length;
    depths.set(line.from, blocks.filter((block) => block.from < first && first < innerEnd(block)).length);
  }
  return depths;
}
