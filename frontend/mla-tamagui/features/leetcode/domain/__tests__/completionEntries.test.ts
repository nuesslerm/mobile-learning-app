import { describe, expect, it } from 'vitest';

import { completionEntriesFor } from '../completionEntries';
import { ProblemCatalog } from '../problemCatalog';

const problems = ProblemCatalog.getAll();
const problem = (id: string) => problems.find((candidate) => candidate.id === id) as (typeof problems)[number];
const labelsOf = (entries: { label: string }[]) => entries.map((entry) => entry.label);

describe('completionEntriesFor', () => {
  it('given every catalog problem with a method and python3, when entries are built, then the method name is present and the parameters equal the first case inputs where cases exist', () => {
    const withMethod = problems.filter((candidate) => candidate.methodName !== null);

    for (const candidate of withMethod) {
      const entries = completionEntriesFor(candidate, 'python');
      expect(labelsOf(entries), candidate.id).toContain(candidate.methodName);
      const first = candidate.testCases?.[0];
      if (first && !candidate.harness) {
        const parameters = entries.filter((entry) => entry.type === 'variable').map((entry) => entry.label).filter((label) => label !== 'self');
        expect(parameters, candidate.id).toEqual(Object.keys(first.inputs));
      }
    }
    expect(withMethod.length).toBeGreaterThan(60);
  });

  it('given reverse-linked-list, when entries are built, then the commented __init__ is not taken and ListNode is offered', () => {
    const entries = completionEntriesFor(problem('reverse-linked-list'), 'python');

    expect(labelsOf(entries)).toEqual(expect.arrayContaining(['reverseList', 'head', 'ListNode', 'Optional']));
    expect(labelsOf(entries)).not.toContain('val');
  });

  it('given the two-sum starter, when entries are built, then nums carries the detail List[int] and the method lists its parameters', () => {
    const entries = completionEntriesFor(problem('two-sum'), 'python');

    expect(entries.find((entry) => entry.label === 'nums')).toEqual({ label: 'nums', type: 'variable', detail: 'List[int]' });
    expect(entries.find((entry) => entry.label === 'twoSum')).toEqual({ label: 'twoSum', type: 'function', detail: '(nums, target)' });
    expect(entries.find((entry) => entry.label === 'self')).toMatchObject({ type: 'variable' });
  });

  it('given a problem with no starter for the language, when entries are built, then the list is empty', () => {
    expect(completionEntriesFor({ starterCode: {}, methodName: 'f' }, 'python')).toEqual([]);
    expect(completionEntriesFor({ starterCode: { python3: 'class A:\n    pass\n' }, methodName: null }, 'python')).toEqual([]);
    expect(completionEntriesFor({ starterCode: { python3: 'class A:\n    pass\n' }, methodName: 'missing' }, 'python')).toEqual([]);
  });

  it('given the JavaScript starter of two-sum, when entries are built, then twoSum, nums and target are present', () => {
    expect(labelsOf(completionEntriesFor(problem('two-sum'), 'javascript'))).toEqual(['twoSum', 'nums', 'target']);
  });

  it('given the C++ starter of two-sum, when entries are built, then the parameters carry their types', () => {
    const entries = completionEntriesFor(problem('two-sum'), 'cpp');

    expect(entries.find((entry) => entry.label === 'nums')).toMatchObject({ detail: 'vector<int>' });
    expect(entries.find((entry) => entry.label === 'target')).toMatchObject({ detail: 'int' });
  });

  it('given a signature with nested brackets and defaults, when entries are built, then each parameter is one entry', () => {
    const entries = completionEntriesFor({ starterCode: { python3: 'class S:\n    def f(self, pairs: Dict[str, List[int]], k: int = 3, *rest) -> None:\n        ' }, methodName: 'f' }, 'python');

    expect(labelsOf(entries)).toEqual(['f', 'self', 'pairs', 'k', 'rest', 'Dict', 'List']);
    expect(entries.find((entry) => entry.label === 'pairs')).toMatchObject({ detail: 'Dict[str, List[int]]' });
  });

  it('given every catalog problem and each language, when entries are built, then every label is a word of at most 64 characters and there are at most 200', () => {
    for (const candidate of problems) {
      for (const language of ['python', 'javascript', 'cpp'] as const) {
        const entries = completionEntriesFor(candidate, language);
        expect(entries.length, candidate.id).toBeLessThanOrEqual(200);
        for (const entry of entries) expect(entry.label, candidate.id).toMatch(/^[A-Za-z_]\w{0,63}$/);
      }
    }
  });
});
