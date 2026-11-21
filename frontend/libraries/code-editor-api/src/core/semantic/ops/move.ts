import { resolveDestination } from '../destination';
import { carryPos, frozenLines, linesOf, shiftPrefix } from '../indent';
import type { Planner, StepOf } from '../plan';
import { statementShapes } from '../verify';

/**
 * Moves the whole lines of a run, the comment lines above it with them, to a destination, at the indent the destination's block has. A move to where the lines already are changes nothing;
 * a destination inside the moved lines is refused.
 */
export const planMove: Planner<StepOf<'moveNode'>> = (step, context) => {
  const source = context.locate(step.node, step.expect);
  if (!source.ok) return source;
  const placed = resolveDestination(step.to, context);
  if (!placed.ok) return placed;
  const { run } = source.value;
  const { extent, indentPrefix, unterminated } = source.value;
  const place = placed.value;
  const { text, root, language } = context.input;

  const point = place.at.from === place.at.to;
  const inside = point ? place.at.from > extent.from && place.at.from < extent.to : place.at.from >= extent.from && place.at.to <= extent.to;
  if (inside) return { ok: false, error: { code: 'invalid', message: 'a node cannot move into itself', path: `steps[${context.step}].to` } };

  const lines = linesOf(text, extent);
  const shifted = shiftPrefix(lines, indentPrefix, place.prefix, frozenLines(text, root, language));
  const adjacent = point && (place.at.from === extent.from || place.at.from === extent.to);
  const none = { edits: [], landed: null, expectation: {} };
  if (adjacent && shifted.every((line, index) => line === lines[index]?.text)) return { ok: true, value: none };

  const composed = place.compose(shifted, unterminated && place.at.from === extent.from);
  return {
    ok: true,
    value: {
      edits: [
        { from: extent.from, to: extent.to, insert: '' },
        { from: place.at.from, to: place.at.to, insert: composed.text },
      ],
      landed: { edit: 1, from: composed.lines.from, to: composed.lines.to },
      expectation: { removed: [extent], writes: { parent: place.parent, shapes: statementShapes(language, run) } },
      follow: { range: { from: extent.from, to: unterminated ? extent.to : extent.to - 1 }, offset: carryPos(lines, shifted, indentPrefix, place.prefix) },
    },
  };
};
