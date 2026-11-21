import type { Change, EditorError, SemanticStep, SemanticStepOp, Target } from '../../ports/protocol';
import type { RunInput, Run } from './model';
import type { Expectation } from './verify';

export type Resolved<T> = { readonly ok: true; readonly value: T } | { readonly ok: false; readonly error: EditorError };

/** What a planner decides for one step: whole-line edits in the planned document, the part of one edit's insert that is the step's own text, and the structure it promises. */
export type StepPlan = {
  readonly edits: readonly Change[];
  readonly landed: { readonly edit: number; readonly from: number; readonly to: number } | null;
  readonly expectation: Expectation;
};

export type PlanContext = {
  /** the document's text and full tree, as they are at the revision the call was made against */
  readonly input: RunInput;
  readonly indentUnit: string;
  /** the whole-line run a target names ('selection' is the live selection, an empty one standing for the statement at the caret); `expect` guards the text of an explicit range */
  locate(target: Target, expect?: string): Resolved<Run>;
};

/** The step of one op: the node steps share variants (`unwrapNode`, `deleteNode` and `duplicateNode` take the same fields), so this is an intersection, not an Extract. */
export type StepOf<Op extends SemanticStepOp> = SemanticStep & { readonly op: Op };

export type Planner<S extends SemanticStep = SemanticStep> = (step: S, context: PlanContext) => Resolved<StepPlan>;
