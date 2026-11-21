import type { Language, Pos, Range, SyntaxNode } from '../../../ports/protocol';
import { classify, familyOf } from '../../syntax';
import { lineStartOf } from '../../syntax/blocks';
import { parentKey } from '../destination';
import { carryPos, depthMap, frozenLines, linesOf, shiftPrefix, type Line } from '../indent';
import { contentEnd, type Run } from '../model';
import type { Planner, StepOf } from '../plan';
import { statementShapes } from '../verify';

const leadingOf = (line: string): string => /^[ \t]*/.exec(line)?.[0] ?? '';
const prefixOfLine = (text: string, pos: Pos): string => leadingOf(text.slice(lineStartOf(text, pos)));
const startsLine = (text: string, node: SyntaxNode): boolean => text.slice(lineStartOf(text, node.from), node.from).trim() === '';

const descendants = (nodes: readonly SyntaxNode[]): SyntaxNode[] => nodes.flatMap((node) => [node, ...descendants(node.children ?? [])]);

// one unit shallower: a whole unit if the prefix ends with one, else a tab, else as many characters as a unit has
function dedent(prefix: string, unit: string): string {
  if (prefix.endsWith(unit)) return prefix.slice(0, -unit.length);
  if (prefix.endsWith('\t')) return prefix.slice(0, -1);
  return prefix.slice(0, Math.max(0, prefix.length - unit.length));
}

// the indent the run's own lines start at: that of the statements around it, or one unit inside the owner of its block when it is the whole block
function baseOf(text: string, language: Language, run: Run, unit: string): string {
  const { parent, owner, extent } = run;
  const around = (parent.children ?? []).find((child) => ['simple', 'compound'].includes(classify(language, child)) && (child.to <= extent.from || child.from >= extent.to) && startsLine(text, child));
  if (around) return prefixOfLine(text, around.from);
  return classify(language, parent) === 'root' ? '' : prefixOfLine(text, (owner ?? parent).from) + unit;
}

type Anchor = { readonly was: string; readonly now: string };

// every line at base plus one unit per block it is inside; a continuation line follows the line its statement began on
function normalise(language: Language, text: string, lines: readonly Line[], frozen: ReadonlySet<Pos>, run: Run, base: string, unit: string): string[] {
  const nodes = descendants(run.run);
  const braces = familyOf(language) === 'braces';
  const blocks = nodes.filter((node) => classify(language, node) === 'block');
  const innerEnd = (block: SyntaxNode) => (braces && text[block.to - 1] === '}' ? block.to - 1 : block.to);
  const depths = depthMap(language, text, blocks, run.extent);
  const braceLines = new Set<Pos>(braces ? blocks.flatMap((block) => [block.from, block.to - 1]) : []);
  // where a statement's own line goes on: up to its end, or for a compound up to its first block
  const zones: Range[] = nodes.flatMap((node) => {
    const kind = classify(language, node);
    if (kind === 'simple') return [{ from: node.from, to: contentEnd(text, node) }];
    if (kind !== 'compound') return [];
    const body = (node.children ?? []).find((child) => classify(language, child) === 'block');
    return [{ from: node.from, to: body ? body.from : contentEnd(text, node) }];
  });
  const continues = (at: Pos) => zones.some((zone) => zone.from < at && at < zone.to && !blocks.some((block) => zone.from <= block.from && block.from < at && at < innerEnd(block)));

  let anchor: Anchor | null = null;
  return lines.map(({ from, text: line }) => {
    if (frozen.has(from) || line.trim() === '') return line;
    const first = from + (line.length - line.trimStart().length);
    if (!braceLines.has(first) && continues(first)) return anchor && line.startsWith(anchor.was) ? anchor.now + line.slice(anchor.was.length) : line;
    const now = base + unit.repeat(depths.get(from) ?? 0);
    anchor = { was: leadingOf(line), now };
    return now + line.trimStart();
  });
}

/**
 * Re-indents the whole lines of a run. With `by` the run moves that many indent units (a negative one dedents) and the code is left to say whether the result still parses. Without it
 * every line gets the indent its depth asks for: the indent of the sibling statements around the run (or one unit inside the block's owner when the run is the whole block) plus one unit
 * for each block the line is inside, a closing brace or an `else` line having its owner's depth. A line that continues a statement keeps its offset to the line the statement began on, and
 * a line that begins inside a multi-line string keeps its text.
 */
export const planReindent: Planner<StepOf<'reindentBlock'>> = (step, context) => {
  const source = context.locate(step.node);
  if (!source.ok) return source;
  const { run, extent, indentPrefix, unterminated, parent } = source.value;
  const { text, root, language } = context.input;
  const lines = linesOf(text, extent);
  const frozen = frozenLines(text, root, language);

  const target = step.by === undefined ? null : step.by >= 0 ? indentPrefix + context.indentUnit.repeat(step.by) : Array.from({ length: -step.by }).reduce<string>((prefix) => dedent(prefix, context.indentUnit), indentPrefix);
  const shifted = target === null ? normalise(language, text, lines, frozen, source.value, baseOf(text, language, source.value, context.indentUnit), context.indentUnit) : shiftPrefix(lines, indentPrefix, target, frozen);
  if (shifted.every((line, index) => line === lines[index]?.text)) return { ok: true, value: { edits: [], landed: null, expectation: {} } };

  const insert = shifted.join('\n') + (unterminated ? '' : '\n');
  return {
    ok: true,
    value: {
      edits: [{ from: extent.from, to: extent.to, insert }],
      landed: { edit: 0, from: 0, to: insert.length },
      expectation: { removed: [extent], writes: { parent: target === null ? parentKey(language, parent) : 'any', shapes: statementShapes(language, run) } },
      follow: { range: { from: extent.from, to: unterminated ? extent.to : extent.to - 1 }, offset: carryPos(lines, shifted) },
    },
  };
};
