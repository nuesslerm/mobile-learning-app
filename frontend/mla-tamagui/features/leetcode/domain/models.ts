/**
 * LeetCode Blind 75 domain models.
 *
 * The single source of truth for the shape of a problem and its catalog. Types
 * are derived from Zod schemas (`z.infer`) so the runtime contract and the
 * compile-time type can never drift: the catalog validates `blind75.json`
 * against `catalogSchema` at load time instead of hand-rolling type guards.
 *
 * This module is pure (no React / React Native / I/O).
 */

import { z } from 'zod';

import { testCaseSchema } from './execution';

export const difficultySchema = z.enum(['easy', 'medium', 'hard']);
export type Difficulty = z.infer<typeof difficultySchema>;

export const nodeTypeSchema = z.enum([
  'ListNode',
  'ListNode[]',
  'TreeNode',
  'TreeNode_by_val',
  'TreeNode_val',
]);
export type NodeType = z.infer<typeof nodeTypeSchema>;

export const problemSchema = z.object({
  id: z.string(),
  leetcodeId: z.number(),
  title: z.string(),
  difficulty: difficultySchema,
  category: z.string(),
  categoryId: z.string(),
  description: z.string(),
  examples: z.string(),
  constraints: z.string(),
  starterCode: z.record(z.string(), z.string()),
  methodName: z.string().nullable(),
  paramTypes: z.record(z.string(), nodeTypeSchema),
  returnType: nodeTypeSchema.nullable(),
  testCases: z.array(testCaseSchema).nullable(),
  hints: z.array(z.string()),
  topics: z.array(z.string()),
  diagram: z.string().optional(),
});
export type Blind75Problem = z.infer<typeof problemSchema>;

export const categorySchema = z.object({
  id: z.string(),
  title: z.string(),
  problems: z.array(problemSchema),
});
export type Blind75Category = z.infer<typeof categorySchema>;

export const catalogSchema = z.object({
  categories: z.array(categorySchema),
});
export type Blind75Catalog = z.infer<typeof catalogSchema>;
