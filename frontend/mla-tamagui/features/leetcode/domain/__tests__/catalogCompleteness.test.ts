import { describe, expect, it } from 'vitest';

import type { Blind75Problem } from '../models';
import { ProblemCatalog } from '../problemCatalog';

const problems = ProblemCatalog.getAll();
const designs = problems.filter((problem) => problem.harness?.kind === 'design');
const roundTrips = problems.filter((problem) => problem.harness?.kind === 'roundTrip');
const cases = (problem: Blind75Problem) => problem.testCases ?? [];
const MAX_HARNESS_CASE_BYTES = 4 * 1024;

const methodsOf = (starter: string): string[] => [...starter.matchAll(/^\s*def (\w+)\(self/gm)].map((match) => match[1] as string).filter((name) => name !== '__init__');
const voidMethodsOf = (starter: string): string[] => [...starter.matchAll(/^\s*def (\w+)\(self[^)]*\)\s*->\s*None\s*:/gm)].map((match) => match[1] as string);

/** A level-order tree list read the way leetcode.com reads it and written out again: a list that is not in its canonical form comes back different. */
function canonicalTree(values: (number | null)[]): (number | null)[] {
  type Branch = { val: number; left?: Branch; right?: Branch };
  if (values.length === 0 || values[0] === null || values[0] === undefined) return [];
  const root: Branch = { val: values[0] };
  const queue = [root];
  let next = 1;
  while (queue.length > 0 && next < values.length) {
    const branch = queue.shift() as Branch;
    const left = values[next++];
    if (left !== null && left !== undefined) {
      branch.left = { val: left };
      queue.push(branch.left);
    }
    const right = next < values.length ? values[next++] : null;
    if (right !== null && right !== undefined) {
      branch.right = { val: right };
      queue.push(branch.right);
    }
  }
  const out: (number | null)[] = [];
  const level: (Branch | undefined)[] = [root];
  while (level.length > 0) {
    const branch = level.shift();
    out.push(branch ? branch.val : null);
    if (branch) level.push(branch.left, branch.right);
  }
  while (out[out.length - 1] === null) out.pop();
  return out;
}

describe('the catalog is complete enough to run', () => {
  it('given every catalog problem, when its cases are counted, then there are at least three', () => {
    for (const problem of problems) expect(cases(problem).length, problem.id).toBeGreaterThanOrEqual(3);
  });

  it('given a design problem, when each case is read, then operations, arguments and expected have one entry each, the first operation is the class, every later one is a starter method, and an entry is null exactly for the constructor and the methods annotated None', () => {
    expect(designs.map((problem) => problem.id).sort()).toEqual(['design-add-and-search-words-data-structure', 'find-median-from-data-stream', 'implement-trie-prefix-tree']);
    for (const problem of designs) {
      const starter = problem.starterCode.python3 ?? '';
      for (const [index, testCase] of cases(problem).entries()) {
        const label = `${problem.id}#${index}`;
        const { operations, arguments: args } = testCase.inputs as { operations: string[]; arguments: unknown[][] };
        const expected = testCase.expected as unknown[];
        expect([operations.length, args.length], label).toEqual([expected.length, expected.length]);
        expect(operations[0], label).toBe(problem.harness?.className);
        for (const [step, name] of operations.entries()) {
          if (step === 0) continue;
          expect(methodsOf(starter), `${label} operation ${step}`).toContain(name);
          expect(expected[step] === null, `${label} operation ${step} (${name})`).toBe(voidMethodsOf(starter).includes(name));
        }
        expect(expected[0], label).toBeNull();
      }
    }
  });

  it('given a round-trip problem, when each case is read, then expected equals the input value and a tree input is in its canonical level-order form', () => {
    expect(roundTrips.map((problem) => problem.id).sort()).toEqual(['encode-and-decode-strings', 'serialize-and-deserialize-binary-tree']);
    for (const problem of roundTrips) {
      for (const [index, testCase] of cases(problem).entries()) {
        const input = Object.values(testCase.inputs);
        expect(input, `${problem.id}#${index}`).toHaveLength(1);
        expect(testCase.expected, `${problem.id}#${index}`).toEqual(input[0]);
        if (Object.values(problem.paramTypes)[0] === 'TreeNode') expect(canonicalTree(input[0] as (number | null)[]), `${problem.id}#${index}`).toEqual(input[0]);
      }
    }
  });

  it('given linked-list-cycle, when each case is read, then pos is -1 or an index inside head and both answers are covered', () => {
    const answers = new Set<unknown>();
    for (const [index, testCase] of cases(ProblemCatalog.getByIdOrThrow('linked-list-cycle')).entries()) {
      const { head, pos } = testCase.inputs as { head: number[]; pos: number };
      expect(Number.isInteger(pos) && pos >= -1 && pos < head.length, `#${index}: pos ${pos} in a list of ${head.length}`).toBe(true);
      answers.add(testCase.expected);
    }
    expect([...answers].sort()).toEqual([false, true]);
  });

  it('given clone-graph, when each case is read, then every neighbour is a node of 1 to n, the graph is symmetric, has no loop or repeated edge and is connected, and expected is the input', () => {
    for (const [index, testCase] of cases(ProblemCatalog.getByIdOrThrow('clone-graph')).entries()) {
      const graph = testCase.inputs.node as number[][];
      const label = `#${index}`;
      expect(testCase.expected, label).toEqual(graph);
      for (const [from, neighbours] of graph.entries()) {
        expect(new Set(neighbours).size, `${label} node ${from + 1} repeats an edge`).toBe(neighbours.length);
        for (const to of neighbours) {
          expect(to >= 1 && to <= graph.length && to !== from + 1, `${label} node ${from + 1} -> ${to}`).toBe(true);
          expect(graph[to - 1], `${label} ${to} lists ${from + 1}`).toContain(from + 1);
        }
      }
      const seen = new Set<number>(graph.length > 0 ? [1] : []);
      const queue = graph.length > 0 ? [1] : [];
      while (queue.length > 0) {
        for (const to of graph[(queue.shift() as number) - 1] ?? []) {
          if (seen.has(to)) continue;
          seen.add(to);
          queue.push(to);
        }
      }
      expect(seen.size, `${label} is connected`).toBe(graph.length);
    }
  });

  it('given every harness problem, when a case is serialised, then it is at most 4 KB so the run card stays readable', () => {
    for (const problem of problems.filter((candidate) => candidate.harness || candidate.paramTypes.node === 'GraphNode')) {
      for (const [index, testCase] of cases(problem).entries()) expect(new TextEncoder().encode(JSON.stringify(testCase)).length, `${problem.id}#${index}`).toBeLessThanOrEqual(MAX_HARNESS_CASE_BYTES);
    }
  });
});
