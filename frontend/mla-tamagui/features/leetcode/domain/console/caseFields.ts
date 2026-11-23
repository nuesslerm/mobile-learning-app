import type { Blind75Problem } from '../models';
import { parameterNames, parseJsonValue } from '../run/customCase';
import type { TestCase } from '../run/execution';

export type CaseFieldTexts = Record<string, string>;

export type CaseFieldCaption = { name: string; caption: string };

export type CaseFieldsResult = { ok: true; testCase: TestCase } | { ok: false; name: string; message: string };

/** A value as JSON text, strings quoted: the console shows `"([)]"`, where `showValue` would print `([)]`. */
export function jsonText(value: unknown): string {
  return value === undefined ? 'null' : (JSON.stringify(value) ?? 'null');
}

/** One caption per parameter: its name, and its node type when the problem builds one from the JSON. */
export function captionsOf(problem: Blind75Problem): CaseFieldCaption[] {
  return parameterNames(problem).map((name) => {
    const type = problem.paramTypes[name];
    return { name, caption: type ? `${name} . ${type}` : name };
  });
}

/** The editable text of every parameter of a case. */
export function fieldTextsOf(testCase: TestCase, names: readonly string[]): CaseFieldTexts {
  return Object.fromEntries(names.map((name) => [name, jsonText(testCase.inputs[name])]));
}

/** The case a custom draft's fields stand for: one JSON value per parameter in parameter order and expected null, or the first field that is not JSON. */
export function parseCaseFields(texts: CaseFieldTexts, names: readonly string[]): CaseFieldsResult {
  const inputs: Record<string, unknown> = {};
  for (const name of names) {
    const text = (texts[name] ?? '').trim();
    const parsed = text === '' ? ({ ok: false } as const) : parseJsonValue(text);
    if (!parsed.ok) {
      const hint = text === '' ? 'is empty' : 'is not JSON: use true, false, null and double quotes';
      return { ok: false, name, message: `${name} ${hint}` };
    }
    inputs[name] = parsed.value;
  }
  return { ok: true, testCase: { inputs, expected: null } };
}
