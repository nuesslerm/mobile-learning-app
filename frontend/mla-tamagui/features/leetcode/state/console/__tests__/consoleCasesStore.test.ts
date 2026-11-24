import { describe, expect, it } from 'vitest';

import { consoleCases } from '../../../domain/console/testcases';
import { ProblemCatalog } from '../../../domain/problemCatalog';
import { useConsoleCasesStore } from '../consoleCasesStore';

const twoSum = ProblemCatalog.getByIdOrThrow('two-sum');
const parentheses = ProblemCatalog.getByIdOrThrow('valid-parentheses');
const casesOf = (problem: typeof twoSum) => consoleCases(problem, useConsoleCasesStore.getState().drafts[problem.id] ?? { items: [], nextKey: 0 });

describe('consoleCasesStore', () => {
  it('Given a draft added on two-sum, When valid-parentheses is read, Then it has none', () => {
    useConsoleCasesStore.getState().reset(twoSum);
    useConsoleCasesStore.getState().reset(parentheses);

    useConsoleCasesStore.getState().addCase(twoSum);

    expect(casesOf(twoSum).filter((entry) => entry.kind === 'custom')).toHaveLength(1);
    expect(casesOf(parentheses).filter((entry) => entry.kind === 'custom')).toHaveLength(0);
  });

  it('Given a draft, When the selected case is removed, Then the previous chip is selected', () => {
    useConsoleCasesStore.getState().reset(twoSum);
    const first = useConsoleCasesStore.getState().addCase(twoSum);
    const second = useConsoleCasesStore.getState().addCase(twoSum);

    useConsoleCasesStore.getState().removeCase(twoSum, second ?? '');

    expect(useConsoleCasesStore.getState().selected[twoSum.id]).toBe(first);
  });

  it('Given custom cases, When reset, Then only bundled chips remain and Case 1 is selected', () => {
    useConsoleCasesStore.getState().addCase(twoSum);

    useConsoleCasesStore.getState().reset(twoSum);

    expect(casesOf(twoSum).every((entry) => entry.kind === 'bundled')).toBe(true);
    expect(useConsoleCasesStore.getState().selected[twoSum.id]).toBe('bundled:0');
  });

  it('Given a draft field typed over, When read, Then the store holds the text', () => {
    useConsoleCasesStore.getState().reset(twoSum);
    const id = useConsoleCasesStore.getState().addCase(twoSum) ?? '';

    useConsoleCasesStore.getState().editField(twoSum.id, id, 'target', '99');

    expect(casesOf(twoSum).find((entry) => entry.id === id)?.fields['target']).toBe('99');
  });

  it('Given the store, Then it holds no persist middleware', () => {
    expect((useConsoleCasesStore as unknown as { persist?: unknown }).persist).toBeUndefined();
  });
});
