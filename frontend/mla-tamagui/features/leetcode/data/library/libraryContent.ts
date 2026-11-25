import type { LibraryContentPort, LibraryRow } from '../../domain/library/library';
import type { Blind75Problem } from '../../domain/models';
import { ProblemCatalog } from '../../domain/problemCatalog';
import collectionProblems from './collectionProblems.json';
import { readLesson } from './lessonReader';
import studyPlan from './neetCode150.json';

const additionalProblems: Readonly<Record<string, Blind75Problem>> = collectionProblems as Record<string, Blind75Problem>;
const entries = new Map(studyPlan.map((item) => [item.id, item]));

const diceProblem: Blind75Problem = {
  id: 'number-of-dice-rolls-with-target-sum',
  leetcodeId: 1155,
  title: 'Number of Dice Rolls With Target Sum',
  difficulty: 'medium',
  category: 'Dynamic Programming',
  categoryId: 'dynamic-programming',
  description: 'You have n dice, and each die has k faces numbered from 1 to k.\n\nGiven three integers n, k, and target, return the number of possible ways to roll the dice, so the sum of the face-up numbers equals target. Since the answer may be too large, return it modulo 10⁹ + 7.',
  examples: '**Example 1:**\n```\nInput: n = 1, k = 6, target = 3\nOutput: 1\nExplanation: You throw one die with 6 faces.\nThere is only one way to get a sum of 3.\n```\n**Example 2:**\n```\nInput: n = 2, k = 6, target = 7\nOutput: 6\nExplanation: You throw two dice, each with 6 faces.\nThere are 6 ways to get a sum of 7.\n```\n**Example 3:**\n```\nInput: n = 30, k = 30, target = 500\nOutput: 222616187\nExplanation: Return the answer modulo 10⁹ + 7.\n```',
  constraints: '- 1 <= n, k <= 30\n- 1 <= target <= 1000',
  starterCode: { javascript: 'function numRollsToTarget(n, k, target) {\n    \n}' },
  methodName: 'numRollsToTarget',
  paramTypes: {},
  returnType: null,
  testCases: [
    { inputs: { n: 1, k: 6, target: 3 }, expected: 1 },
    { inputs: { n: 2, k: 6, target: 7 }, expected: 6 },
  ],
  hints: ['Count the ways to reach each sum after rolling one more die.'],
  topics: ['Dynamic Programming'],
};

const problems = new Map([...ProblemCatalog.getAll(), ...Object.values(additionalProblems), diceProblem].map((problem) => [problem.id, problem]));

export function findLibraryProblem(id: string) {
  return problems.get(id);
}

export const libraryRows: LibraryRow[] = Array.from(problems.values(), ({ id, leetcodeId, title, difficulty }) => ({
  id, number: leetcodeId, title, difficulty,
})).sort((a, b) => a.number - b.number);

export const bundledLibrary: LibraryContentPort = {
  rows: libraryRows,
  studyPlan,
  lesson(id) {
    const entry = entries.get(id);
    return entry && readLesson(entry.topicId, id);
  },
  problem: findLibraryProblem,
};
