import { describe, expect, it } from 'vitest';

import { ProblemCatalog } from '../../problemCatalog';
import { addDraft, consoleCases, EMPTY_DRAFTS, MAX_CUSTOM_CASES, removeDraft, resetDrafts, editDraft, selectedCase, type ConsoleDrafts } from '../testcases';

const problem = (() => {
  const found = ProblemCatalog.getById('valid-parentheses');
  if (!found) throw new Error('no problem');
  return found;
})();
const bundledCount = problem.testCases?.length ?? 0;

const withDrafts = (count: number): ConsoleDrafts => {
  let drafts = EMPTY_DRAFTS;
  for (let i = 0; i < count; i += 1) {
    const added = addDraft(problem, drafts, 'bundled:0');
    if (!added) throw new Error('refused');
    drafts = added.drafts;
  }
  return drafts;
};

describe('consoleCases', () => {
  it('given valid-parentheses, when the cases are read, then the bundled chips are Case 1..n in catalog order', () => {
    const cases = consoleCases(problem, EMPTY_DRAFTS);
    expect(cases).toHaveLength(bundledCount);
    expect(cases.map((c) => c.label)).toEqual(Array.from({ length: bundledCount }, (_, i) => `Case ${i + 1}`));
    expect(cases.every((c) => c.kind === 'bundled')).toBe(true);
  });

  it('given three drafts and the middle one removed, when the cases are read, then labels stay contiguous and the other ids are unchanged', () => {
    const drafts = withDrafts(3);
    const middle = drafts.items[1]?.id ?? '';
    const { drafts: after } = removeDraft(problem, drafts, middle);
    const custom = consoleCases(problem, after).slice(bundledCount);
    expect(custom.map((c) => c.id)).toEqual([drafts.items[0]?.id, drafts.items[2]?.id]);
    expect(custom.map((c) => c.label)).toEqual([`Case ${bundledCount + 1}`, `Case ${bundledCount + 2}`]);
  });
});

describe('addDraft', () => {
  it('given a selected case, when a draft is added, then it holds that case\'s JSON field texts', () => {
    const added = addDraft(problem, EMPTY_DRAFTS, 'bundled:2');
    const selected = selectedCase(consoleCases(problem, EMPTY_DRAFTS), 'bundled:2');
    expect(added?.drafts.items[0]?.fields).toEqual(selected?.fields);
    expect(added?.id).toBe('custom:0');
  });

  it('given ten drafts, when an eleventh is added, then it is refused', () => {
    expect(addDraft(problem, withDrafts(MAX_CUSTOM_CASES), 'bundled:0')).toBeNull();
  });

  it('given an edited draft, when the cases are read, then the field holds the new text', () => {
    const edited = editDraft(withDrafts(1), 'custom:0', 's', '"()"');
    expect(consoleCases(problem, edited).at(-1)?.fields).toEqual({ s: '"()"' });
  });
});

describe('removeDraft and resetDrafts', () => {
  it('given the last draft selected, when it is removed, then the selection falls to the chip before it', () => {
    const drafts = withDrafts(2);
    expect(removeDraft(problem, drafts, 'custom:1').selectedId).toBe('custom:0');
    expect(removeDraft(problem, withDrafts(1), 'custom:0').selectedId).toBe(`bundled:${bundledCount - 1}`);
  });

  it('given drafts, when reset, then only bundled cases remain and Case 1 is selected', () => {
    const reset = resetDrafts(problem);
    expect(consoleCases(problem, reset.drafts)).toHaveLength(bundledCount);
    expect(reset.selectedId).toBe('bundled:0');
  });

  it('given an id that no longer exists, when the selection is read, then it is the first case', () => {
    expect(selectedCase(consoleCases(problem, EMPTY_DRAFTS), 'custom:9')?.id).toBe('bundled:0');
  });
});
