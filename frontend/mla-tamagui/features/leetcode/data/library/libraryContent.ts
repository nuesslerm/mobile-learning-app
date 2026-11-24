import type { LibraryContentPort, LibraryRow, LibrarySnapshot } from '../../domain/library/library';
import type { Blind75Problem } from '../../domain/models';
import { ProblemCatalog } from '../../domain/problemCatalog';
import { companies, sampleSolutions, sampleSubmissionCode, sampleSubmissions, twoSumHints } from './problemActivity';

const recordedRows: LibraryRow[] = [
  { id: 'two-sum', number: 1, title: 'Two Sum', difficulty: 'easy', acceptance: '58.2%', frequency: '100.0%', solved: true },
  { id: 'add-two-numbers', number: 2, title: 'Add Two Numbers', difficulty: 'medium', acceptance: '49.7%', frequency: '96.5%' },
  { id: 'longest-substring-without-repeating-characters', number: 3, title: 'Longest Substring Without Repeating Characters', difficulty: 'medium', acceptance: '40.2%', frequency: '97.3%', solved: true },
  { id: 'median-of-two-sorted-arrays', number: 4, title: 'Median of Two Sorted Arrays', difficulty: 'hard', acceptance: '48.0%', frequency: '97.0%' },
  { id: 'longest-palindromic-substring', number: 5, title: 'Longest Palindromic Substring', difficulty: 'medium', acceptance: '36.5%', frequency: '96.5%' },
  { id: 'zigzag-conversion', number: 6, title: 'Zigzag Conversion', difficulty: 'medium' },
  { id: 'reverse-integer', number: 7, title: 'Reverse Integer', difficulty: 'medium' },
  { id: 'two-sum-bsts', number: 1214, title: 'Two Sum BSTs', difficulty: 'medium', acceptance: '68.2%', frequency: '0%' },
  { id: 'sum-of-two-integers', number: 371, title: 'Sum of Two Integers', difficulty: 'medium', acceptance: '55.7%', frequency: '64.1%', solved: true },
  { id: 'two-sum-less-than-k', number: 1099, title: 'Two Sum Less Than K', difficulty: 'easy', acceptance: '62.2%', frequency: '0%' },
  { id: 'two-sum-iii-data-structure-design', number: 170, title: 'Two Sum III - Data structure design', difficulty: 'easy', acceptance: '38.3%', frequency: '0%' },
  { id: 'minimum-index-sum-of-two-lists', number: 599, title: 'Minimum Index Sum of Two Lists', difficulty: 'easy', acceptance: '57.0%', frequency: '0%' },
  { id: 'number-of-dice-rolls-with-target-sum', number: 1155, title: 'Number of Dice Rolls With Target Sum', difficulty: 'medium' },
];

export const libraryRows: LibraryRow[] = [
  ...recordedRows,
  ...ProblemCatalog.getAll().filter((problem) => !recordedRows.some((row) => row.number === problem.leetcodeId)).map((problem) => ({
    id: problem.id,
    number: problem.leetcodeId,
    title: problem.title,
    difficulty: problem.difficulty,
  })),
];

const twoSumDescription = 'You are given an array of integers nums and an integer target, return indices of the two numbers such that they add up to target.\n\nYou may assume that each input would have exactly one solution, and you may not use the same element twice.\n\nYou can return the answer in any order.';

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

export function findLibraryProblem(id: string) {
  if (id === diceProblem.id) return diceProblem;
  const problem = ProblemCatalog.getById(id);
  return problem?.id === 'two-sum' ? { ...problem, description: twoSumDescription } : problem;
}

export const sampleLibrary: LibrarySnapshot = {
  lastOpened: diceProblem.id,
  lists: [
    { id: 'favorite', name: 'Favorite', description: '', private: false, problemIds: [] },
    { id: 'python-done', name: 'Python Done', description: '', private: false, problemIds: ['two-sum'] },
    { id: 'python-todo', name: 'Python Todo', description: '', private: false, problemIds: [] },
  ],
};

export const bundledLibrary: LibraryContentPort = {
  rows: libraryRows,
  problem: findLibraryProblem,
  submissions: (id) => id === 'two-sum' ? sampleSubmissions : [],
  submission: (id, submissionId) => Promise.resolve(id === 'two-sum' ? sampleSubmissions.find((row) => row.id === submissionId) : undefined),
  submissionCode: sampleSubmissionCode,
  solutions: (id) => id === 'two-sum' ? sampleSolutions : [],
  companies,
  hints: (problem) => problem.id === 'two-sum' ? twoSumHints : problem.hints,
};
