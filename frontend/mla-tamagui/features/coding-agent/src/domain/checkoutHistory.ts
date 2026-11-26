import { z } from 'zod';

export const checkoutHistory = z.object({
  branch: z.string().nullable(),
  head: z.string().nullable(),
  nextOffset: z.number().int().positive().nullable(),
  commits: z.array(
    z.object({
      sha: z.string(),
      shortSha: z.string(),
      subject: z.string(),
      author: z.string(),
      authoredAtMs: z.number(),
      parent: z.string().nullable(),
    })
  ),
});
export type CheckoutHistory = z.infer<typeof checkoutHistory>;
export type HistoryCursor = { head: string; offset: number };

export class CheckoutHeadChanged extends Error {}

const changedFile = z.object({
  path: z.string(),
  status: z.string(),
  additions: z.number().optional(),
  deletions: z.number().optional(),
  binary: z.boolean(),
});
export const checkoutChanges = z.object({
  head: z.string().nullable(),
  branch: z.string().nullable(),
  comparison: z
    .object({
      base: z.string(),
      ahead: z.number(),
      limited: z.boolean(),
      files: z.array(changedFile),
    })
    .nullable(),
});
export const checkoutCommit = z.object({
  body: z.string(),
  initial: z.boolean(),
  limited: z.boolean(),
  files: z.array(changedFile),
});
export type CheckoutChanges = z.infer<typeof checkoutChanges>;
export type CheckoutCommit = z.infer<typeof checkoutCommit>;
