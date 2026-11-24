/**
 * A comment attached to one specific golden-screenshot capture — the
 * `GOLDEN_SCREENSHOT_COMMENTS` ORDS auto-REST table
 * (created once by the since-removed wf command `infra oracle db add-golden-screenshot-comments`).
 *
 * `screenshotKey` identifies which capture a comment belongs to
 * (`<suite>::<case-name>::<generatedAt>`) rather than a numeric row id, so a
 * client posting a comment never has to look up a foreign key first — it
 * already has all three fields from the `HealthReport` it's rendering. See
 * `buildScreenshotKey` below, and the migration's own doc comment for why
 * this shape was chosen.
 */

import { z } from 'zod';

export const goldenScreenshotCommentSchema = z.object({
  id: z.number(),
  screenshotKey: z.string(),
  author: z.string().optional(),
  commentText: z.string(),
  createdAt: z.string(),
});
export type GoldenScreenshotComment = z.infer<
  typeof goldenScreenshotCommentSchema
>;

/** Matches `GOLDEN_SCREENSHOT_COMMENTS.SCREENSHOT_KEY` exactly — keep in sync with the migration. */
export function buildScreenshotKey(
  suite: string,
  name: string,
  generatedAt: string
): string {
  return `${suite}::${name}::${generatedAt}`;
}
