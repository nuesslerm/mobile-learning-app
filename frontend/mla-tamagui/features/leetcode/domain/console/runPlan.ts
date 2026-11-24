import type { Blind75Problem } from '../models';
import type { RunCodeParams } from '../ports';
import { parameterNames } from '../run/customCase';
import { runParamsFor } from '../run/runParams';
import { parseCaseFields } from './caseFields';
import { consoleCases, selectedCase, type ConsoleDrafts } from './testcases';

export type RunMode = 'run' | 'submit';

/** A case the run sends, with the chip it came from, in the order it is sent. */
export type PlannedCase = { id: string; label: string; inputs: Record<string, unknown> };

export type RunPlan = {
  mode: RunMode;
  lang: string;
  params: RunCodeParams;
  cases: PlannedCase[];
  /** A run of a typed case: it has no expected value and no verdict. */
  custom: boolean;
};

export type RunPlanOutcome = { ok: true; plan: RunPlan } | { ok: false; reason: 'no-cases' } | { ok: false; reason: 'bad-field'; name: string; message: string };

export type PlanInput = { problem: Blind75Problem; lang: string; code: string; mode: RunMode; drafts: ConsoleDrafts; selectedId: string | null };

/** Run is the selected chip, Submit is the problem's bundled set whatever is selected; both build their params with `runParamsFor`. */
export function planRun({ problem, lang, code, mode, drafts, selectedId }: PlanInput): RunPlanOutcome {
  const bundled = problem.testCases ?? [];
  if (bundled.length === 0) return { ok: false, reason: 'no-cases' };
  if (mode === 'submit') {
    const cases = consoleCases(problem, drafts).filter((entry) => entry.kind === 'bundled');
    return {
      ok: true,
      plan: { mode, lang, params: runParamsFor(problem, lang, code), custom: false, cases: cases.map((entry) => ({ id: entry.id, label: entry.label, inputs: entry.kind === 'bundled' ? entry.testCase.inputs : {} })) },
    };
  }
  const chosen = selectedCase(consoleCases(problem, drafts), selectedId);
  if (!chosen) return { ok: false, reason: 'no-cases' };
  if (chosen.kind === 'bundled') {
    return { ok: true, plan: { mode, lang, params: runParamsFor(problem, lang, code, [chosen.testCase]), custom: false, cases: [{ id: chosen.id, label: chosen.label, inputs: chosen.testCase.inputs }] } };
  }
  const parsed = parseCaseFields(chosen.fields, parameterNames(problem));
  if (!parsed.ok) return { ok: false, reason: 'bad-field', name: parsed.name, message: parsed.message };
  return { ok: true, plan: { mode, lang, params: runParamsFor(problem, lang, code, [parsed.testCase]), custom: true, cases: [{ id: chosen.id, label: chosen.label, inputs: parsed.testCase.inputs }] } };
}
