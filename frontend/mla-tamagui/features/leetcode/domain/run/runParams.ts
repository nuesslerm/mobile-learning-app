import type { RunCodeParams } from '../ports';
import type { Blind75Problem } from '../models';

/** What a run of `code` against a catalog problem needs: its cases, and what the harness must know to build and compare their values. */
export function runParamsFor(problem: Blind75Problem, lang: string, code: string): RunCodeParams {
  return {
    lang,
    code,
    methodName: problem.methodName ?? '',
    testCases: problem.testCases ?? [],
    paramTypes: problem.paramTypes,
    returnType: problem.returnType,
    inPlace: problem.inPlaceParam ?? null,
  };
}
