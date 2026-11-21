import { describe, expect, it } from 'vitest';

import { ProblemCatalog } from '../problemCatalog';
import { runParamsFor } from '../run/runParams';

const problems = ProblemCatalog.getAll();
const withCases = problems.filter((p) => p.testCases !== null);
const paramsOf = (starter: string, method: string): string[] => {
  const signature = starter.match(new RegExp(`def ${method}\\(self,?\\s*([^)]*)\\)`))?.[1] ?? '';
  return signature.split(',').map((part) => part.trim().split(':')[0]?.trim() ?? '').filter(Boolean);
};

describe('the catalog the harness runs', () => {
  it('given every problem with cases, when its inputs are read, then their keys are the starter method parameters in order', () => {
    // Given the problems with bundled cases, When each case's input keys are compared with the starter's parameters
    for (const problem of withCases) {
      const starter = problem.starterCode.python3 ?? '';
      const expected = paramsOf(starter, problem.methodName ?? '');
      for (const testCase of problem.testCases ?? []) expect(Object.keys(testCase.inputs), problem.id).toEqual(expected);
    }
  });

  it('given a problem with paramTypes, when its keys are read, then each is a parameter and a tree-by-value follows a tree', () => {
    for (const problem of problems.filter((p) => Object.keys(p.paramTypes).length > 0)) {
      const names = paramsOf(problem.starterCode.python3 ?? '', problem.methodName ?? '');
      for (const [name, type] of Object.entries(problem.paramTypes)) {
        expect(names, `${problem.id}.${name}`).toContain(name);
        if (type === 'TreeNode_by_val') expect(Object.values(problem.paramTypes)).toContain('TreeNode');
      }
    }
  });

  it('given every starter that returns None from a Solution method, when inPlaceParam is read, then it names an existing parameter', () => {
    const inPlace = problems.filter((p) => /\)\s*->\s*None\s*:/.test(p.starterCode.python3 ?? '') && p.methodName !== null && !/^\s*def __init__/m.test(p.starterCode.python3 ?? ''));
    expect(inPlace.map((p) => p.id).sort()).toEqual(['reorder-list', 'rotate-image', 'set-matrix-zeroes']);
    for (const problem of inPlace) expect(paramsOf(problem.starterCode.python3 ?? '', problem.methodName ?? ''), problem.id).toContain(problem.inPlaceParam);
    expect(problems.filter((p) => p.inPlaceParam !== undefined).map((p) => p.id).sort()).toEqual(['reorder-list', 'rotate-image', 'set-matrix-zeroes']);
  });

  it('given the bundled cases, when their compare modes and values are read, then modes are known and no value is a float', () => {
    const floats: string[] = [];
    const walk = (value: unknown, id: string) => {
      if (typeof value === 'number' && !Number.isInteger(value)) floats.push(id);
      if (Array.isArray(value)) value.forEach((item) => walk(item, id));
      if (value && typeof value === 'object') Object.values(value).forEach((item) => walk(item, id));
    };
    for (const problem of withCases) {
      for (const testCase of problem.testCases ?? []) {
        expect(['exact', 'sorted', 'setOfSorted', undefined]).toContain(testCase.compareMode);
        walk(testCase.inputs, problem.id);
        walk(testCase.expected, problem.id);
      }
    }
    expect(floats).toEqual([]);
  });

  it('given the catalog, when counted, then 72 problems, 64 with cases and 721 cases, and the eight without are the known list', () => {
    expect(problems).toHaveLength(72);
    expect(withCases).toHaveLength(64);
    expect(withCases.reduce((n, p) => n + (p.testCases?.length ?? 0), 0)).toBe(721);
    expect(problems.filter((p) => p.testCases === null).map((p) => p.id).sort()).toMatchInlineSnapshot(`
      [
        "clone-graph",
        "design-add-and-search-words-data-structure",
        "encode-and-decode-strings",
        "find-median-from-data-stream",
        "implement-trie-prefix-tree",
        "linked-list-cycle",
        "reorder-list",
        "serialize-and-deserialize-binary-tree",
      ]
    `);
  });

  it('given a problem, when its run params are built, then they carry the cases and what the harness needs to build values', () => {
    const rotate = ProblemCatalog.getById('rotate-image');
    expect(rotate && runParamsFor(rotate, 'python3', 'code')).toMatchObject({ lang: 'python3', code: 'code', methodName: 'rotate', inPlace: 'matrix', returnType: null, paramTypes: {} });
    expect(rotate && runParamsFor(rotate, 'python3', 'code').testCases.length).toBeGreaterThan(0);
  });
});
