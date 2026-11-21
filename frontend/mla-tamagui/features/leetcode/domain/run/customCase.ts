import type { Blind75Problem } from '../models';
import type { TestCase } from './execution';

/** The input is echoed into the result card, so a pasted document is refused. */
export const MAX_CUSTOM_INPUT_CHARS = 20_000;

export type CustomInputProblem =
  | { kind: 'empty' }
  | { kind: 'tooLong' }
  | { kind: 'count'; names: string[]; got: number }
  | { kind: 'json'; line: number; name: string };

export type CustomInput = { ok: true; testCase: TestCase } | { ok: false; problem: CustomInputProblem };

/** The arguments a problem takes, in order: the first bundled case's keys (none for a problem without cases). */
export function parameterNames(problem: Blind75Problem): string[] {
  return Object.keys(problem.testCases?.[0]?.inputs ?? {});
}

const CURLY_DOUBLE = /[“”]/g;
const CURLY_SINGLE = /[‘’]/g;

/**
 * One JSON value per line in parameter order, the format of the leetcode.com Run. The case it builds has an explicit `expected: null`,
 * because the backend's `TestCase.expected` has no serde default; the key order of `inputs` is the harness's positional argument order.
 */
export function parseCustomInput(text: string, names: readonly string[]): CustomInput {
  if (text.length > MAX_CUSTOM_INPUT_CHARS) return { ok: false, problem: { kind: 'tooLong' } };
  const lines = text
    .split(/\r?\n/)
    .map((value, index) => ({ value: value.trim(), line: index + 1 }))
    .filter((entry) => entry.value !== '');
  if (lines.length === 0) return { ok: false, problem: { kind: 'empty' } };
  if (lines.length !== names.length) return { ok: false, problem: { kind: 'count', names: [...names], got: lines.length } };
  const values: unknown[] = [];
  for (const [index, entry] of lines.entries()) {
    try {
      values.push(JSON.parse(entry.value.replace(CURLY_DOUBLE, '"').replace(CURLY_SINGLE, "'")));
    } catch {
      return { ok: false, problem: { kind: 'json', line: entry.line, name: names[index] ?? '' } };
    }
  }
  return { ok: true, testCase: { inputs: Object.fromEntries(names.map((name, index) => [name, values[index]])), expected: null } };
}

/** The text a case reads back as: the inverse of `parseCustomInput`, and the field's starting value. */
export function writeCustomInput(testCase: TestCase): string {
  return Object.values(testCase.inputs)
    .map((value) => JSON.stringify(value))
    .join('\n');
}

/** What to tell the learner under the field, in one line. */
export function customInputMessage(problem: CustomInputProblem): string {
  switch (problem.kind) {
    case 'empty':
      return 'Type the arguments, one JSON value per line';
    case 'tooLong':
      return `Keep the input under ${MAX_CUSTOM_INPUT_CHARS} characters`;
    case 'count':
      return `Expected ${problem.names.length} ${problem.names.length === 1 ? 'value' : 'values'} (${problem.names.join(', ')}), got ${problem.got}`;
    case 'json':
      return `Line ${problem.line} (${problem.name}) is not JSON: use true, false, null and double quotes`;
  }
}
