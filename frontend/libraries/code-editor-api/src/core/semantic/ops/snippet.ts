import { resolveDestination } from '../destination';
import type { Planner, StepOf } from '../plan';

/**
 * Inserts text as whole lines at a destination. The snippet is dedented by the indent of its first line, then each line takes the destination's indent on top of its own relative one, and
 * a final newline is ensured. The snippet is not parsed on its own: what it must be is statements (or comments) in the destination's block, which refuses an `else:` that would extend the
 * `if` before it. Lines inside a multi-line string of the snippet are indented like any other.
 */
export const planSnippet: Planner<StepOf<'insertSnippet'>> = (step, context) => {
  const placed = resolveDestination(step.at, context);
  if (!placed.ok) return placed;
  const place = placed.value;

  const lines = step.text.split('\n');
  if (lines.at(-1) === '') lines.pop();
  const first = lines.find((line) => line.trim() !== '') ?? '';
  const base = /^[ \t]*/.exec(first)?.[0] ?? '';
  const indented = lines.map((line) => (line.trim() === '' ? '' : place.prefix + (line.startsWith(base) ? line.slice(base.length) : line.trimStart())));
  const composed = place.compose(indented);
  return {
    ok: true,
    value: {
      edits: [{ from: place.at.from, to: place.at.to, insert: composed.text }],
      landed: { edit: 0, from: composed.lines.from, to: composed.lines.to },
      expectation: { writes: { parent: place.parent } },
    },
  };
};
