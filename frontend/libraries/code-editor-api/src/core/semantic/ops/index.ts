import { SEMANTIC_STEP_OPS, type SemanticStep, type SemanticStepOp } from '../../../ports/protocol';
import type { Planner, StepOf } from '../plan';
import { planDelete } from './delete';

type PlannerOf<Op extends SemanticStepOp> = Planner<StepOf<Op>>;

/** The node steps this engine can plan: a step with no planner is `unsupported`, and hello does not announce it. */
export const PLANNERS: { readonly [Op in SemanticStepOp]?: PlannerOf<Op> } = { deleteNode: planDelete };

export const plannedOps: readonly SemanticStepOp[] = SEMANTIC_STEP_OPS.filter((op) => PLANNERS[op] !== undefined);

export const plannerFor = (step: SemanticStep): Planner | undefined => PLANNERS[step.op] as Planner | undefined;
