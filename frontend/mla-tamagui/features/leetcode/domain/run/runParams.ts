import type { RunCodeParams } from '../ports';
import type { Blind75Problem } from '../models';
import type { TestCase } from './execution';

/**
 * What a run of `code` against a catalog problem needs: its cases, and what the harness must know to build and compare their values.
 * `cases` replaces the bundled ones (a typed case still builds its list and tree values from the problem's own types).
 */
export function runParamsFor(problem: Blind75Problem, lang: string, code: string, cases: TestCase[] = problem.testCases ?? []): RunCodeParams {
  return {
    lang,
    code,
    methodName: problem.methodName ?? '',
    testCases: cases,
    paramTypes: problem.paramTypes,
    returnType: problem.returnType,
    inPlace: problem.inPlaceParam ?? null,
    harness: problem.harness ?? null,
  };
}
