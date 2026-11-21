/**
 * LeetCode Blind 75 Problem Catalog
 *
 * Reads the static blind75.json and
 * provides typed lookup functions. The raw JSON is validated against the
 * `domain/` Zod schema at module load, so a malformed dataset fails loudly at
 * startup rather than producing silently-wrong types downstream.
 *
 * This is the feature's `data/` layer: it depends only on `domain/`.
 */

import {
  catalogSchema,
  type Blind75Category,
  type Blind75Problem,
  type Difficulty,
} from './models';
import { Logger } from '@lib/logging';

import blind75Data from './blind75.json';
function loadCatalog() {
  try {
    const parsed = catalogSchema.parse(blind75Data);
    const problemCount = parsed.categories.reduce(
      (n, c) => n + c.problems.length,
      0
    );
    Logger.info('leetcode.catalog.load', {
      categoryCount: parsed.categories.length,
      problemCount,
      parseOk: true,
    });
    return parsed;
  } catch (err) {
    // Fail loud (a malformed dataset is a build-time bug) — but log first so the
    // failure is queryable, not just a raw startup throw.
    Logger.withError(err)
      .withMetadata({
        parseOk: false,
      })
      .error('leetcode.catalog.load');
    throw err;
  }
}
const { categories } = loadCatalog();
const allProblems: Blind75Problem[] = categories.flatMap((c) => c.problems);
const byId = new Map(allProblems.map((p) => [p.id, p]));
export const ProblemCatalog = {
  getAll: (): Blind75Problem[] => {
    Logger.info('leetcode.catalog.getAll', {
      count: allProblems.length,
    });
    return allProblems;
  },
  getById: (id: string): Blind75Problem | undefined => {
    const p = byId.get(id);
    Logger.info('leetcode.catalog.getById', {
      id,
      found: Boolean(p),
    });
    return p;
  },
  getByIdOrThrow: (id: string): Blind75Problem => {
    const p = byId.get(id);
    if (!p) {
      Logger.error('leetcode.catalog.getByIdOrThrow', {
        id,
        found: false,
      });
      throw new Error(`Problem not found: ${id}`);
    }
    Logger.info('leetcode.catalog.getByIdOrThrow', {
      id,
      found: true,
    });
    return p;
  },
  getCategories: (): Blind75Category[] => {
    Logger.info('leetcode.catalog.getCategories', {
      count: categories.length,
    });
    return categories;
  },
  getByDifficulty: (difficulty: Difficulty): Blind75Problem[] => {
    const matches = allProblems.filter((p) => p.difficulty === difficulty);
    Logger.info('leetcode.catalog.getByDifficulty', {
      difficulty,
      count: matches.length,
    });
    return matches;
  },
  getByCategory: (categoryId: string): Blind75Category | undefined => {
    const c = categories.find((cat) => cat.id === categoryId);
    Logger.info('leetcode.catalog.getByCategory', {
      categoryId,
      found: Boolean(c),
    });
    return c;
  },
};
