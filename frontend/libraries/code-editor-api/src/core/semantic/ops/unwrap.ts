import type { EditorError, NotANodeReason } from '../../../ports/protocol';
import { classify, familyOf } from '../../syntax';
import { lineEndOf, lineStartOf } from '../../syntax/blocks';
import { parentKey } from '../destination';
import { carryPos, frozenLines, linesOf, shiftPrefix } from '../indent';
import { blocksOf, contentEnd, endsLine } from '../model';
import type { Planner, StepOf } from '../plan';
import { statementShapes } from '../verify';

/**
 * Replaces a compound statement with the whole lines of its one block, one indent unit shallower: the header lines and the closing brace go. A compound with more than one block (an `if`
 * with an `else`, a `try` with its clauses), with something after its block (a `do` loop's condition) or with a body on its header's line is refused.
 */
export const planUnwrap: Planner<StepOf<'unwrapNode'>> = (step, context) => {
  const source = context.locate(step.node, step.expect);
  if (!source.ok) return source;
  const { run, extent, indentPrefix, unterminated, parent } = source.value;
  const { text, root, language } = context.input;
  const [head] = run;
  const tail = run.at(-1);
  const named = head && tail ? { from: head.from, to: contentEnd(text, tail) } : extent;
  const refuse = (reason: NotANodeReason, message: string): { readonly ok: false; readonly error: EditorError } => ({ ok: false, error: { code: 'not-a-node', message, range: named, reason, step: context.step } });

  const statements = run.filter((node) => classify(language, node) !== 'comment');
  const compound = statements[0];
  if (statements.length !== 1 || !compound || classify(language, compound) !== 'compound') return refuse('not-unwrappable', 'only one compound statement can be unwrapped');
  const blocks = blocksOf(language, compound);
  const block = blocks[0];
  const inside = (block?.children ?? []).filter((child) => ['simple', 'compound'].includes(classify(language, child)));
  const first = inside[0];
  if (blocks.length !== 1 || !block || !first) return refuse('not-unwrappable', 'the statement has no single block of statements to take out');

  const braces = familyOf(language) === 'braces';
  const close = block.to - 1;
  if (braces && contentEnd(text, compound) !== block.to) return refuse('not-unwrappable', 'something follows the block of the statement');
  const openLineEnd = lineEndOf(text, block.from);
  const firstLine = lineStartOf(text, first.from);
  const blockPrefix = text.slice(firstLine, first.from);
  const closeLine = braces ? lineStartOf(text, close) : extent.to;
  const bodyShares = firstLine <= lineStartOf(text, block.from) || blockPrefix.trim() !== '' || !endsLine(text.slice(block.from + 1, openLineEnd), language);
  const closerShares = braces && (closeLine <= openLineEnd || text.slice(closeLine, close).trim() !== '' || !endsLine(text.slice(close + 1, lineEndOf(text, close)), language));
  if (bodyShares || closerShares) return refuse('shares-line', 'the block shares a line with its header or its closing brace');

  const body = { from: openLineEnd + 1, to: closeLine };
  const lines = linesOf(text, body);
  const shifted = shiftPrefix(lines, blockPrefix, indentPrefix, frozenLines(text, root, language));
  const insert = shifted.join('\n') + (unterminated ? '' : '\n');
  return {
    ok: true,
    value: {
      edits: [{ from: extent.from, to: extent.to, insert }],
      landed: { edit: 0, from: 0, to: insert.length },
      expectation: { removed: [extent], writes: { parent: parentKey(language, parent), shapes: statementShapes(language, block.children ?? []) } },
      follow: { range: { from: body.from, to: unterminated && !braces ? body.to : body.to - 1 }, offset: carryPos(lines, shifted) },
    },
  };
};
