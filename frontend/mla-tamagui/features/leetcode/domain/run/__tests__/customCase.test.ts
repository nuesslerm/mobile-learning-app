import { describe, expect, it } from 'vitest';

import type { Blind75Problem } from '../../models';
import { parameterNames, parseJsonValue } from '../customCase';
import type { TestCase } from '../execution';

const TWO_SUM = ['nums', 'target'];

const problemWith = (testCases: TestCase[] | null): Blind75Problem => ({
  id: 'fixture',
  leetcodeId: 0,
  title: 'Fixture',
  difficulty: 'easy',
  category: 'Fixtures',
  categoryId: 'fixtures',
  description: '',
  examples: '',
  constraints: '',
  starterCode: {},
  methodName: 'solve',
  paramTypes: {},
  returnType: null,
  testCases,
  hints: [],
  topics: [],
});

describe('parameterNames', () => {
  it('given a problem whose first case has two inputs, when its names are read, then they are those keys in order', () => {
    expect(parameterNames(problemWith([{ inputs: { nums: [1], target: 2 }, expected: [0] }]))).toEqual(TWO_SUM);
  });

  it('given a problem whose testCases is null, when its names are read, then there are none', () => {
    expect(parameterNames(problemWith(null))).toEqual([]);
  });

  it('given a problem with an empty case list, when its names are read, then there are none', () => {
    expect(parameterNames(problemWith([]))).toEqual([]);
  });
});

describe('parseJsonValue', () => {
  it('given text with curly quotes, when it is read, then it reads as JSON', () => {
    expect(parseJsonValue('“abc”')).toEqual({ ok: true, value: 'abc' });
  });

  it('given broken JSON, when it is read, then it is not ok', () => {
    expect(parseJsonValue('[1,')).toEqual({ ok: false });
  });
});
