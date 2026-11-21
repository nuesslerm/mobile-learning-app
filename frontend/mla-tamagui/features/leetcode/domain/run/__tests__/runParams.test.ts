import { describe, expect, it } from 'vitest';

import type { Blind75Problem } from '../../models';
import type { TestCase } from '../execution';
import { runParamsFor } from '../runParams';

const bundled: TestCase[] = [
  { inputs: { matrix: [[1, 2], [3, 4]] }, expected: [[3, 1], [4, 2]] },
  { inputs: { matrix: [[1]] }, expected: [[1]] },
];

const inPlaceProblem: Blind75Problem = {
  id: 'fixture-in-place',
  leetcodeId: 0,
  title: 'Fixture',
  difficulty: 'medium',
  category: 'Fixtures',
  categoryId: 'fixtures',
  description: '',
  examples: '',
  constraints: '',
  starterCode: {},
  methodName: 'rotate',
  inPlaceParam: 'matrix',
  paramTypes: {},
  returnType: null,
  testCases: bundled,
  hints: [],
  topics: [],
};

describe('runParamsFor', () => {
  it('given an in-place problem and a typed case, when run params are built with that case, then testCases is exactly that case and paramTypes, returnType and inPlace are carried', () => {
    const typed: TestCase = { inputs: { matrix: [[5, 6], [7, 8]] }, expected: null };
    expect(runParamsFor(inPlaceProblem, 'python3', 'code', [typed])).toEqual({
      lang: 'python3',
      code: 'code',
      methodName: 'rotate',
      testCases: [typed],
      paramTypes: {},
      returnType: null,
      inPlace: 'matrix',
    });
  });

  it('given a problem that builds linked lists, when run params are built with a typed case, then the node types travel with it', () => {
    const lists: Blind75Problem = { ...inPlaceProblem, inPlaceParam: undefined, methodName: 'reverseList', paramTypes: { head: 'ListNode' }, returnType: 'ListNode' };
    const typed: TestCase = { inputs: { head: [1, 2, 3] }, expected: null };
    expect(runParamsFor(lists, 'python3', 'code', [typed])).toMatchObject({ testCases: [typed], paramTypes: { head: 'ListNode' }, returnType: 'ListNode', inPlace: null });
  });

  it("given no cases argument, when run params are built, then testCases is the problem's bundled cases", () => {
    expect(runParamsFor(inPlaceProblem, 'python3', 'code').testCases).toBe(bundled);
  });

  it('given a problem with no bundled cases and no cases argument, when run params are built, then testCases is empty', () => {
    expect(runParamsFor({ ...inPlaceProblem, testCases: null }, 'python3', 'code').testCases).toEqual([]);
  });

  it('given a problem with no bundled cases and a typed case, when run params are built, then the typed case is the only one', () => {
    const typed: TestCase = { inputs: { matrix: [[1]] }, expected: null };
    expect(runParamsFor({ ...inPlaceProblem, testCases: null }, 'python3', 'code', [typed]).testCases).toEqual([typed]);
  });
});
