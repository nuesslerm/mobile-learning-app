import { describe, expect, it } from 'vitest';

import type { Blind75Problem } from '../../models';
import { ProblemCatalog } from '../../problemCatalog';
import { runParamsFor } from '../../run/runParams';
import { planRun, type RunPlan } from '../runPlan';
import { addDraft, consoleCases, editDraft, EMPTY_DRAFTS } from '../testcases';

const catalog = (id: string): Blind75Problem => {
  const found = ProblemCatalog.getById(id);
  if (!found) throw new Error(`no problem ${id}`);
  return found;
};
const parentheses = catalog('valid-parentheses');

const planned = (outcome: ReturnType<typeof planRun>): RunPlan => {
  if (!outcome.ok) throw new Error(`refused ${outcome.reason}`);
  return outcome.plan;
};

const noCases: Blind75Problem = { ...parentheses, id: 'fixture', testCases: null };

describe('planRun', () => {
  it('given case 3 selected, when Run is planned, then the params hold that one bundled case with its expected and compareMode', () => {
    const plan = planned(planRun({ problem: parentheses, lang: 'python3', code: 'x', mode: 'run', drafts: EMPTY_DRAFTS, selectedId: 'bundled:2' }));
    expect(plan.params.testCases).toEqual([parentheses.testCases?.[2]]);
    expect(plan.custom).toBe(false);
    expect(plan.cases.map((c) => c.label)).toEqual(['Case 3']);
  });

  it('given a custom case selected, when Run is planned, then the case has expected null', () => {
    const added = addDraft(parentheses, EMPTY_DRAFTS, 'bundled:0');
    const drafts = editDraft(added?.drafts ?? EMPTY_DRAFTS, added?.id ?? '', 's', '"(("');
    const plan = planned(planRun({ problem: parentheses, lang: 'python3', code: 'x', mode: 'run', drafts, selectedId: added?.id ?? null }));
    expect(plan.params.testCases).toEqual([{ inputs: { s: '((' }, expected: null }]);
    expect(plan.custom).toBe(true);
  });

  it('given custom chips and a selection, when Submit is planned, then the params hold the bundled set exactly', () => {
    const added = addDraft(parentheses, EMPTY_DRAFTS, 'bundled:0');
    const plan = planned(planRun({ problem: parentheses, lang: 'python3', code: 'x', mode: 'submit', drafts: added?.drafts ?? EMPTY_DRAFTS, selectedId: added?.id ?? null }));
    expect(plan.params.testCases).toEqual(parentheses.testCases);
    expect(plan.cases).toHaveLength(parentheses.testCases?.length ?? -1);
  });

  it('given merge-two-sorted-lists, when planned, then the params are the ones runParamsFor builds', () => {
    const problem = catalog('merge-two-sorted-lists');
    const plan = planned(planRun({ problem, lang: 'python3', code: 'c', mode: 'submit', drafts: EMPTY_DRAFTS, selectedId: null }));
    expect(plan.params).toEqual(runParamsFor(problem, 'python3', 'c'));
    expect(plan.params.paramTypes).toBeTruthy();
  });

  it('given the catalog identifies sample cases, when the console opens and Submit is planned, then the console numbers custom cases after the samples and Submit still runs the full suite', () => {
    const problem = catalog('merge-two-sorted-lists');
    const added = addDraft(problem, EMPTY_DRAFTS, 'bundled:2');
    const drafts = added?.drafts ?? EMPTY_DRAFTS;

    expect(consoleCases(problem, drafts).map((entry) => [entry.label, entry.kind])).toEqual([
      ['Case 1', 'bundled'],
      ['Case 2', 'bundled'],
      ['Case 3', 'bundled'],
      ['Case 4', 'custom'],
    ]);
    const submission = planned(planRun({ problem, lang: 'javascript', code: 'code', mode: 'submit', drafts, selectedId: added?.id ?? null }));
    expect(submission.params.testCases).toEqual(problem.testCases);
    expect(submission.cases).toHaveLength(problem.testCases?.length ?? 0);
  });

  it('given a problem with no bundled cases, when planned, then it is refused no-cases', () => {
    expect(planRun({ problem: noCases, lang: 'python3', code: '', mode: 'run', drafts: EMPTY_DRAFTS, selectedId: null })).toEqual({ ok: false, reason: 'no-cases' });
    expect(planRun({ problem: noCases, lang: 'python3', code: '', mode: 'submit', drafts: EMPTY_DRAFTS, selectedId: null })).toEqual({ ok: false, reason: 'no-cases' });
  });

  it('given a bad custom field, when Run is planned, then it is refused bad-field with the parameter name', () => {
    const added = addDraft(parentheses, EMPTY_DRAFTS, 'bundled:0');
    const drafts = editDraft(added?.drafts ?? EMPTY_DRAFTS, added?.id ?? '', 's', "'(");
    const outcome = planRun({ problem: parentheses, lang: 'python3', code: '', mode: 'run', drafts, selectedId: added?.id ?? null });
    expect(outcome).toMatchObject({ ok: false, reason: 'bad-field', name: 's' });
  });
});
