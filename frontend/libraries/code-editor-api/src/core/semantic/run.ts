import type { DocumentHost, TreeSnapshot } from '../../ports/host';
import type { EditorError, Pos, Range, Revision, SelRange, Step, SyntaxNode, Target } from '../../ports/protocol';
import { applyChanges, excerpt } from '../changes';
import { planBatch, rebaseSelect, type Plan, type ResolvedStep } from '../plan';
import { runTreeJob, type TreeJobClock } from '../treeJob';
import { isSemanticStep } from '../validate';
import { DEFAULT_INDENT_UNIT } from './indent';
import { locateRun, type Run, type RunInput } from './model';
import { plannerFor } from './ops';
import type { PlanContext, Resolved, StepPlan } from './plan';
import { newErrors, verifyStructure, type Expectation } from './verify';

/** A host that can parse the document and another text: what a node step needs. */
export type ParsingHost = DocumentHost & Required<Pick<DocumentHost, 'tree' | 'treeOf'>>;
export const canParse = (host: DocumentHost): host is ParsingHost => host.tree !== undefined && host.treeOf !== undefined;

export type Failure = { readonly ok: false; readonly error: EditorError };
export type SemanticEnv = { readonly clock: TreeJobClock; readonly budgetMs: number };
export type SemanticOutcome = { readonly ok: true; readonly plan: Plan } | Failure;

const fail = (error: EditorError): Failure => ({ ok: false, error });

// the live selection goes with the text of the step that carried it, unless the call said where it ends up
function followed(host: ParsingHost, plan: Plan, follows: readonly (StepPlan['follow'] | undefined)[]): SelRange | null {
  const main = host.selection().ranges[host.selection().main];
  if (!main) return null;
  for (const [step, follow] of follows.entries()) {
    const landed = plan.landed[step];
    const inside = (pos: Pos) => follow !== undefined && pos >= follow.range.from && pos <= follow.range.to;
    if (follow && landed && inside(main.anchor) && inside(main.head)) return { anchor: landed.from + follow.offset(main.anchor), head: landed.from + follow.offset(main.head) };
  }
  return null;
}

/**
 * Plans a call made of node steps (and at most one select step) against the head, and checks the result before it is committed: the document is parsed in slices, the steps are planned
 * into one simultaneous change list, the result text is parsed through `host.treeOf`, and no syntax error may be new and the structure must be the one the steps promised.
 * Nothing is applied here. The caller commits `plan` only if the head is still the one the call began at; a keystroke that landed during a parse ends it as `structure-changed`.
 */
export async function runSemanticApply(host: ParsingHost, steps: readonly Step[], env: SemanticEnv): Promise<SemanticOutcome> {
  const { clock, budgetMs } = env;
  const base: Revision = clock.head();
  const text = host.doc.slice(0, host.doc.length);
  const source = host.tree();

  // the head is checked before completeness: a parse that stopped because the text moved is `structure-changed`, not a timeout
  const parse = async (snapshot: TreeSnapshot, length: number): Promise<{ readonly ok: true; readonly root: SyntaxNode } | Failure> => {
    const job = await runTreeJob(snapshot, length, budgetMs, clock);
    host.sync();
    if (clock.head() !== base) return fail({ code: 'structure-changed', message: 'the code changed while the step was being planned', head: clock.head() });
    const root = job.complete ? snapshot.read({ from: 0, to: length }, 'full') : null;
    return root ? { ok: true, root } : fail({ code: 'parse-timeout', message: `the code was not parsed within ${budgetMs} ms`, budgetMs });
  };

  const before = await parse(source, text.length);
  if (!before.ok) return before;
  const input: RunInput = { text, root: before.root, language: source.language };
  const selection = host.selection().ranges[host.selection().main] ?? { anchor: 0, head: 0 };
  const live: Range = { from: Math.min(selection.anchor, selection.head), to: Math.max(selection.anchor, selection.head) };
  const indentUnit = host.indentUnit?.() ?? DEFAULT_INDENT_UNIT;

  const locate = (step: number, target: Target, expect: string | undefined): Resolved<Run> => {
    const range = target === 'selection' ? live : target;
    if (range.to > text.length) return fail({ code: 'out-of-range', message: `step ${step} names text outside the document (length ${text.length})`, length: text.length, step });
    const actual = text.slice(range.from, range.to);
    if (expect !== undefined && actual !== expect) return fail({ code: 'text-mismatch', message: `step ${step} expected other text`, step, expected: excerpt(expect), actual: excerpt(actual) });
    const located = locateRun(input, range, { caret: target === 'selection' });
    return located.ok ? { ok: true, value: located.value } : fail({ ...located.error, step });
  };

  const resolved: ResolvedStep[] = [];
  const expectations: (Expectation | null)[] = [];
  const follows: (StepPlan['follow'] | undefined)[] = [];
  for (const [index, step] of steps.entries()) {
    if (step.op === 'select') {
      const select = rebaseSelect(step, text.length, (pos) => pos, () => ({ code: 'out-of-range', message: `step ${index} names text outside the document (length ${text.length})`, length: text.length, step: index }));
      if ('code' in select) return fail(select);
      resolved.push(select);
      expectations.push(null);
      follows.push(undefined);
      continue;
    }
    const planner = isSemanticStep(step) ? plannerFor(step) : undefined;
    if (!isSemanticStep(step) || !planner) return fail({ code: 'unsupported', message: `this editor has no step ${step.op}`, what: `step:${step.op}` });
    const context: PlanContext = { step: index, input, indentUnit, locate: (target, expect) => locate(index, target, expect) };
    const planned = planner(step, context);
    if (!planned.ok) return fail(planned.error);
    resolved.push({ op: 'splice', edits: planned.value.edits, landed: planned.value.landed });
    expectations.push(planned.value.expectation);
    follows.push(planned.value.follow);
  }

  const batch = planBatch(host.doc, resolved);
  if (!batch.ok) return fail(batch.error);
  if (batch.plan.changes.length === 0) return { ok: true, plan: batch.plan };
  const carried = batch.plan.select === null ? followed(host, batch.plan, follows) : null;
  const plan: Plan = carried ? { ...batch.plan, select: carried } : batch.plan;

  const result = applyChanges(text, plan.changes);
  const after = await parse(host.treeOf(result), result.length);
  if (!after.ok) return after;
  const broken = newErrors(before.root, after.root, plan.changes);
  if (broken.length > 0) return fail({ code: 'would-not-parse', message: 'the edit would leave a syntax error in the code', errors: broken, reason: 'syntax' });
  const verified = verifyStructure({ language: source.language, base: before.root, result: after.root, changes: plan.changes, written: plan.written, expectations });
  return verified.ok ? { ok: true, plan } : fail(verified.error);
}
