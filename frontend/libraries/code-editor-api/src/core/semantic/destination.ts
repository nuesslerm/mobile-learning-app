import type { Destination, EditorError, Language, Pos, Range, SyntaxNode } from '../../ports/protocol';
import { classify } from '../syntax';
import { lineEndOf, lineStartOf } from '../syntax/blocks';
import { contentEnd, locateCompound } from './model';
import type { PlanContext, Resolved } from './plan';

/** Where whole lines go: the document range they replace, the indent they take, and the block they will sit in. */
export type Placement = {
  /** a point at a line start, or (an empty `{}` that is opened up) the inside of the braces */
  readonly at: Range;
  readonly prefix: string;
  /** the root, or the position of the block's opening mark in the planned document */
  readonly parent: 'root' | Pos;
  /** The text that replaces `at` for these lines (indented, none holding a newline), and where in it the lines themselves are. With `open` nothing follows them in the result, so they keep no final newline. */
  compose(lines: readonly string[], open?: boolean): { readonly text: string; readonly lines: Range };
};

export const parentKey = (language: Language, parent: SyntaxNode): 'root' | Pos => (classify(language, parent) === 'root' ? 'root' : parent.from);

/** Lines inserted at a line start; after the last line of a document that has no final newline they come after one, and the document stays without one. */
export function insertionAt(text: string, offset: Pos, prefix: string, parent: 'root' | Pos): Placement {
  const lead = offset === text.length && text.length > 0 && !text.endsWith('\n') ? '\n' : '';
  return {
    at: { from: offset, to: offset },
    prefix,
    parent,
    compose(lines, open = false) {
      const inserted = lead + lines.join('\n') + (lead === '' && !open ? '\n' : '');
      return { text: inserted, lines: { from: lead.length, to: inserted.length } };
    },
  };
}

const indentOfLine = (text: string, pos: Pos): string => /^[ \t]*/.exec(text.slice(lineStartOf(text, pos)))?.[0] ?? '';

const failed = (error: EditorError): Resolved<Placement> => ({ ok: false, error });

/**
 * Where a destination puts whole lines. `before` and `after` are relative to the whole-line run the reference names (its leading comments included) and take its indent; `into` is the
 * start or the end of one of the statement blocks of a compound statement, at the indent of the statement there, or an indent deeper than the owner's when the braces are empty.
 */
export function resolveDestination(destination: Destination, context: PlanContext): Resolved<Placement> {
  const { text, language } = context.input;
  if ('before' in destination || 'after' in destination) {
    const reference = context.locate('before' in destination ? destination.before : destination.after);
    if (!reference.ok) return reference;
    const { extent, indentPrefix, parent } = reference.value;
    return { ok: true, value: insertionAt(text, 'before' in destination ? extent.from : extent.to, indentPrefix, parentKey(language, parent)) };
  }

  const refuse = (reason: 'not-in-block' | 'shares-line' | 'damaged', message: string) => failed({ code: 'not-a-node', message, range: destination.into, reason, step: context.step });
  const compound = locateCompound(context.input, destination.into);
  if (!compound.ok) return failed({ ...compound.error, step: context.step });
  const block = compound.value.blocks[destination.block ?? 0];
  if (!block) return refuse('not-in-block', 'the statement has no such block to move into: it is a braceless body or a block of something else');

  const statements = (block.children ?? []).filter((child) => ['simple', 'compound'].includes(classify(language, child)));
  const edge = destination.at === 'start' ? statements[0] : statements.at(-1);
  if (edge) {
    const reference = context.locate({ from: edge.from, to: contentEnd(text, edge) });
    if (!reference.ok) return failed(reference.error.code === 'not-a-node' ? { ...reference.error, range: destination.into } : reference.error);
    const { extent, indentPrefix } = reference.value;
    return { ok: true, value: insertionAt(text, destination.at === 'start' ? extent.from : extent.to, indentPrefix, block.from) };
  }

  if (language === 'python') return refuse('damaged', 'the block holds no statement');
  const ownerPrefix = indentOfLine(text, compound.value.node.from);
  const prefix = ownerPrefix + context.indentUnit;
  const close = block.to - 1;
  const openLineEnd = lineEndOf(text, block.from);
  if (close <= openLineEnd) {
    if (text.slice(block.from + 1, close).trim() !== '') return refuse('shares-line', 'something sits between the braces on their line');
    return {
      ok: true,
      value: {
        at: { from: block.from + 1, to: close },
        prefix,
        parent: block.from,
        compose(lines) {
          const body = lines.join('\n');
          return { text: `\n${body}\n${ownerPrefix}`, lines: { from: 1, to: body.length + 2 } };
        },
      },
    };
  }
  const closeLine = lineStartOf(text, close);
  if (text.slice(closeLine, close).trim() !== '') return refuse('shares-line', 'something sits in front of the closing brace on its line');
  return { ok: true, value: insertionAt(text, destination.at === 'start' ? openLineEnd + 1 : closeLine, prefix, block.from) };
}
