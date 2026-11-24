import type { Blind75Problem } from '../models';
import { parameterNames } from '../run/customCase';
import type { TestCase } from '../run/execution';
import { fieldTextsOf, type CaseFieldTexts } from './caseFields';

export const MAX_CUSTOM_CASES = 10;

export type CustomDraft = { id: string; fields: CaseFieldTexts };

/** The custom cases of one problem: ids come from `nextKey` and are never reused, so a chip keeps its id when another is deleted. */
export type ConsoleDrafts = { items: CustomDraft[]; nextKey: number };

export type ConsoleCase =
  | { id: string; label: string; kind: 'bundled'; testCase: TestCase; fields: CaseFieldTexts }
  | { id: string; label: string; kind: 'custom'; fields: CaseFieldTexts };

export const EMPTY_DRAFTS: ConsoleDrafts = { items: [], nextKey: 0 };

export function bundledCaseIdentity(index: number) {
  return { id: `bundled:${index}`, label: `Case ${index + 1}` };
}

/** Sample cases in catalog order, then custom cases. IDs stay stable as custom labels shift. */
export function consoleCases(problem: Blind75Problem, drafts: ConsoleDrafts): ConsoleCase[] {
  const names = parameterNames(problem);
  const bundled = (problem.testCases ?? []).slice(0, problem.sampleCaseCount).map(
    (testCase, index): ConsoleCase => ({ ...bundledCaseIdentity(index), kind: 'bundled', testCase, fields: fieldTextsOf(testCase, names) })
  );
  const custom = drafts.items.map((draft, index): ConsoleCase => ({ id: draft.id, label: `Case ${bundled.length + index + 1}`, kind: 'custom', fields: draft.fields }));
  return [...bundled, ...custom];
}

/** The chip the id names, or the first one when it is gone. */
export function selectedCase(cases: readonly ConsoleCase[], selectedId: string | null): ConsoleCase | null {
  return cases.find((entry) => entry.id === selectedId) ?? cases[0] ?? null;
}

/** A new custom case seeded with the selected case's values; null at the cap or with no case to copy. */
export function addDraft(problem: Blind75Problem, drafts: ConsoleDrafts, selectedId: string | null): { drafts: ConsoleDrafts; id: string } | null {
  if (drafts.items.length >= MAX_CUSTOM_CASES) return null;
  const from = selectedCase(consoleCases(problem, drafts), selectedId);
  if (!from) return null;
  const id = `custom:${drafts.nextKey}`;
  return { drafts: { items: [...drafts.items, { id, fields: { ...from.fields } }], nextKey: drafts.nextKey + 1 }, id };
}

export function editDraft(drafts: ConsoleDrafts, id: string, name: string, text: string): ConsoleDrafts {
  return { ...drafts, items: drafts.items.map((draft) => (draft.id === id ? { ...draft, fields: { ...draft.fields, [name]: text } } : draft)) };
}

/** Takes a custom case away; the selection moves to the chip before it. */
export function removeDraft(problem: Blind75Problem, drafts: ConsoleDrafts, id: string): { drafts: ConsoleDrafts; selectedId: string | null } {
  const before = consoleCases(problem, drafts);
  const at = before.findIndex((entry) => entry.id === id);
  const next = { ...drafts, items: drafts.items.filter((draft) => draft.id !== id) };
  const previous = at > 0 ? before[at - 1] : before[at + 1];
  return { drafts: next, selectedId: previous && previous.id !== id ? previous.id : null };
}

/** Every custom case dropped and Case 1 selected. */
export function resetDrafts(problem: Blind75Problem): { drafts: ConsoleDrafts; selectedId: string | null } {
  return { drafts: EMPTY_DRAFTS, selectedId: consoleCases(problem, EMPTY_DRAFTS)[0]?.id ?? null };
}
