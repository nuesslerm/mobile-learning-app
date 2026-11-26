import { z } from 'zod';

export const checkoutBranches = z.object({
  branches: z.array(z.string()),
  current: z.string().nullable(),
  truncated: z.boolean(),
});
export type CheckoutBranches = z.infer<typeof checkoutBranches>;

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
  orig_path: z.string().optional(),
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
      baseOid: z.string().optional(),
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

export const checkoutFile = z.discriminatedUnion('kind', [
  z.object({
    kind: z.literal('text'),
    original: z.string().nullable(),
    modified: z.string().nullable(),
  }),
  z.object({ kind: z.literal('binary') }),
  z.object({ kind: z.literal('tooLarge') }),
]);
export type CheckoutFile = z.infer<typeof checkoutFile>;
export type CheckoutFileRequest = {
  head: string;
  base?: string;
  path: string;
  oldPath?: string;
};
