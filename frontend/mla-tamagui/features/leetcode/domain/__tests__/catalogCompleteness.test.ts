import { describe, expect, it } from 'vitest';

import raw from '../blind75.json';
import { problemSchema, type Blind75Problem } from '../models';
import { ProblemCatalog } from '../problemCatalog';

const problems = ProblemCatalog.getAll();
const designs = problems.filter((problem) => problem.harness?.kind === 'design');
const roundTrips = problems.filter((problem) => problem.harness?.kind === 'roundTrip');
const cases = (problem: Blind75Problem) => problem.testCases ?? [];
const MAX_HARNESS_CASE_BYTES = 4 * 1024;
const LANGUAGES = ['cpp', 'golang', 'java', 'javascript', 'python3', 'typescript'];

/** The leetcode.com problem numbers of the Blind 75 list. */
const BLIND_75 = [
  1, 3, 5, 11, 15, 19, 20, 21, 23, 33, 39, 48, 49, 53, 54, 55, 56, 57, 62, 70, 73, 76, 79, 91, 98, 100, 102, 104, 105, 121, 124, 125, 128, 133, 139, 141, 143, 152, 153, 190, 191, 198, 200, 206, 207, 208, 211, 212, 213, 217, 226, 230, 235, 238, 242, 252, 253, 261, 268, 269, 271, 295, 297, 300, 322, 323, 338, 347, 371, 417, 424, 435, 572, 647, 1143,
];

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

  it('given the Blind 75 list, when each of its leetcode ids is looked up, then it is in the catalog and the only catalog problem outside the list is combination-sum-iv', () => {
    expect(BLIND_75).toHaveLength(75);
    expect(new Set(BLIND_75).size).toBe(BLIND_75.length);
    const ids = new Set(problems.map((problem) => problem.leetcodeId));
    expect(BLIND_75.filter((id) => !ids.has(id))).toEqual([]);
    expect(problems.filter((problem) => !BLIND_75.includes(problem.leetcodeId)).map((problem) => problem.id)).toEqual(['combination-sum-iv']);
  });

  it('given the catalog, when ids and leetcode ids are collected, then neither has a duplicate', () => {
    expect(new Set(problems.map((problem) => problem.id)).size).toBe(problems.length);
    expect(new Set(problems.map((problem) => problem.leetcodeId)).size).toBe(problems.length);
  });

  it('given every problem, when its starter languages are read, then python3, javascript, typescript, java, cpp and golang are all present and each starter names the method', () => {
    for (const problem of problems) {
      expect(Object.keys(problem.starterCode).sort(), problem.id).toEqual(LANGUAGES);
      const method = problem.methodName ?? '';
      expect(problem.starterCode.python3, problem.id).toMatch(new RegExp(`def ${method}\\(`));
      for (const language of LANGUAGES) expect((problem.starterCode[language] ?? '').toLowerCase(), `${problem.id} ${language}`).toContain(method.toLowerCase());
    }
  });

  it('given every diagram, when its node labels are read, then none holds a parenthesis, which breaks the Mermaid render', () => {
    for (const problem of problems) {
      const labels = [...(problem.diagram ?? '').matchAll(/\[([^\]]*)\]|\{([^}]*)\}|\|([^|]*)\|/g)].map((match) => match[1] ?? match[2] ?? match[3] ?? '');
      expect(labels.filter((label) => /[()]/.test(label)), problem.id).toEqual([]);
    }
  });

  it('given alien-dictionary, when each case is read, then a non-empty answer lists each letter of the words once', () => {
    for (const [index, testCase] of cases(ProblemCatalog.getByIdOrThrow('alien-dictionary')).entries()) {
      const letters = [...new Set((testCase.inputs.words as string[]).join(''))].sort();
      const answer = testCase.expected as string;
      if (answer !== '') expect([...answer].sort(), `#${index}`).toEqual(letters);
    }
  });

  it('given longest-palindromic-substring, when each case is read, then the answer is a palindrome that sits in the input', () => {
    for (const [index, testCase] of cases(ProblemCatalog.getByIdOrThrow('longest-palindromic-substring')).entries()) {
      const answer = testCase.expected as string;
      expect(testCase.inputs.s as string, `#${index}`).toContain(answer);
      expect([...answer].reverse().join(''), `#${index}`).toBe(answer);
    }
  });

  it('given the catalog data, when the keys of each problem are read, then none is a key the schema strips', () => {
    const known = Object.keys(problemSchema.shape);
    for (const category of raw.categories) {
      for (const problem of category.problems) expect(Object.keys(problem).filter((key) => !known.includes(key)), problem.id).toEqual([]);
    }
  });
});
