import type { Planner, StepOf } from '../plan';

/** Removes the whole lines of the run, the comment lines that travel with it included. */
export const planDelete: Planner<StepOf<'deleteNode'>> = (step, { locate }) => {
  const located = locate(step.node, step.expect);
  if (!located.ok) return located;
  const { extent } = located.value;
  return { ok: true, value: { edits: [{ from: extent.from, to: extent.to, insert: '' }], landed: null, expectation: { removed: [extent] } } };
};
