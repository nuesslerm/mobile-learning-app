import { describe, expect, it } from 'vitest';

import type { Blind75Problem } from '../../models';
import { ProblemCatalog } from '../../problemCatalog';
import { customInputMessage, MAX_CUSTOM_INPUT_CHARS, parameterNames, parseCustomInput, writeCustomInput } from '../customCase';
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

describe('parseCustomInput', () => {
  it('given the two-sum names and an array line then a number line, when parsed, then inputs are nums and target in that key order and expected is null', () => {
    const parsed = parseCustomInput('[2,7,11,15]\n9', TWO_SUM);
    expect(parsed).toEqual({ ok: true, testCase: { inputs: { nums: [2, 7, 11, 15], target: 9 }, expected: null } });
    expect(parsed.ok && Object.keys(parsed.testCase.inputs)).toEqual(TWO_SUM);
    expect(parsed.ok && 'expected' in parsed.testCase).toBe(true);
  });

  it('given CRLF line endings, a trailing newline and blank lines between values, when parsed, then the same case comes out', () => {
    expect(parseCustomInput('[2,7,11,15]\r\n\r\n  9  \r\n\r\n', TWO_SUM)).toEqual(parseCustomInput('[2,7,11,15]\n9', TWO_SUM));
  });

  it('given one line for a two-parameter problem, when parsed, then the problem is count with the names and got 1, and the message names the parameters', () => {
    const parsed = parseCustomInput('[2,7,11,15]', TWO_SUM);
    expect(parsed).toEqual({ ok: false, problem: { kind: 'count', names: TWO_SUM, got: 1 } });
    expect(!parsed.ok && customInputMessage(parsed.problem)).toBe('Expected 2 values (nums, target), got 1');
  });

  it('given three lines for a one-parameter problem, when parsed, then the message says one value', () => {
    const parsed = parseCustomInput('[1,\n2,\n3]', ['nums']);
    expect(!parsed.ok && customInputMessage(parsed.problem)).toBe('Expected 1 value (nums), got 3');
  });

  it('given an unclosed array on the first line, when parsed, then the problem is json on line 1 named nums', () => {
    expect(parseCustomInput('[1,2\n9', TWO_SUM)).toEqual({ ok: false, problem: { kind: 'json', line: 1, name: 'nums' } });
  });

  it('given a bad value after a blank line, when parsed, then the line is the one the learner sees in the field', () => {
    expect(parseCustomInput('[1,2]\n\nnine', TWO_SUM)).toEqual({ ok: false, problem: { kind: 'json', line: 3, name: 'target' } });
  });

  it.each([['None'], ['True'], ["'abc'"]])('given the Python spelling %s, when parsed, then it is a json problem and the message says to use null, true and double quotes', (spelling) => {
    const parsed = parseCustomInput(`[1,2]\n${spelling}`, TWO_SUM);
    expect(parsed).toEqual({ ok: false, problem: { kind: 'json', line: 2, name: 'target' } });
    expect(!parsed.ok && customInputMessage(parsed.problem)).toBe('Line 2 (target) is not JSON: use true, false, null and double quotes');
  });

  it('given curly quotes typed by a phone keyboard around a string argument, when parsed, then they read as straight quotes', () => {
    expect(parseCustomInput('“abcabcbb”', ['s'])).toEqual({ ok: true, testCase: { inputs: { s: 'abcabcbb' }, expected: null } });
  });

  it('given a curly apostrophe inside a string, when parsed, then it reads as a straight one', () => {
    expect(parseCustomInput('"it’s"', ['s'])).toEqual({ ok: true, testCase: { inputs: { s: "it's" }, expected: null } });
  });

  it.each([[''], ['   \n \r\n\t']])('given empty or whitespace-only text %j, when parsed, then the problem is empty', (text) => {
    const parsed = parseCustomInput(text, TWO_SUM);
    expect(parsed).toEqual({ ok: false, problem: { kind: 'empty' } });
    expect(!parsed.ok && customInputMessage(parsed.problem)).toBe('Type the arguments, one JSON value per line');
  });

  it('given text over the limit, when parsed, then the problem is tooLong, and text at the limit is still read', () => {
    const atLimit = `"${'a'.repeat(MAX_CUSTOM_INPUT_CHARS - 2)}"`;
    expect(parseCustomInput(atLimit, ['s']).ok).toBe(true);
    const parsed = parseCustomInput(`${atLimit} `, ['s']);
    expect(parsed).toEqual({ ok: false, problem: { kind: 'tooLong' } });
    expect(!parsed.ok && customInputMessage(parsed.problem)).toContain(String(MAX_CUSTOM_INPUT_CHARS));
  });

  it('given a value that is an object, when parsed, then it stays the one argument it is', () => {
    expect(parseCustomInput('{"a": [1, null]}', ['config'])).toEqual({ ok: true, testCase: { inputs: { config: { a: [1, null] } }, expected: null } });
  });
});

describe('writeCustomInput', () => {
  it('given a case, when it is written, then each argument is one JSON line in key order', () => {
    expect(writeCustomInput({ inputs: { s: 'a b', nums: [1, null], flag: true }, expected: 1 })).toBe('"a b"\n[1,null]\ntrue');
  });

  it('given every bundled case of every problem that has cases, when its inputs are written and parsed back with the problem names, then the inputs are deep-equal and in the same order', () => {
    // Given the real catalog: no count is pinned, the loop covers whatever problems have cases
    const withCases = ProblemCatalog.getAll().filter((problem) => problem.testCases !== null);
    expect(withCases.length).toBeGreaterThan(0);
    const mismatches: string[] = [];
    for (const problem of withCases) {
      for (const [index, testCase] of (problem.testCases ?? []).entries()) {
        // When the case is written as the field's text and parsed again
        const parsed = parseCustomInput(writeCustomInput(testCase), parameterNames(problem));
        // Then the same arguments come back
        const same = parsed.ok && JSON.stringify(Object.entries(parsed.testCase.inputs)) === JSON.stringify(Object.entries(testCase.inputs));
        if (!same) mismatches.push(`${problem.id}#${index}`);
      }
    }
    expect(mismatches).toEqual([]);
  });
});
