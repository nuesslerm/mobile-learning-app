import { insertionAt, parentKey } from '../destination';
import { linesOf } from '../indent';
import type { Planner, StepOf } from '../plan';
import { statementShapes } from '../verify';

/** Copies the whole lines of a run, the comment lines above it included, right after it at its indent. */
export const planDuplicate: Planner<StepOf<'duplicateNode'>> = (step, context) => {
  const source = context.locate(step.node, step.expect);
  if (!source.ok) return source;
  const { run, extent, indentPrefix, parent } = source.value;
  const { text, language } = context.input;
  const place = insertionAt(text, extent.to, indentPrefix, parentKey(language, parent));
  const composed = place.compose(linesOf(text, extent).map((line) => line.text));
  return {
    ok: true,
    value: {
      edits: [{ from: place.at.from, to: place.at.to, insert: composed.text }],
      landed: { edit: 0, from: composed.lines.from, to: composed.lines.to },
      expectation: { writes: { parent: place.parent, shapes: statementShapes(language, run) } },
    },
  };
};
