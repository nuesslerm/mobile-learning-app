// features/coding-agent/src/contracts/dispatchRuns.ts
//
// Zod schemas for the opencode-log dispatch-runs API. `DispatchRunMeta` is the
// list summary; `DispatchRunDetail` adds the raw JSON lines as the server
// stored them. `parseDispatchRunLine` is the per-line guard for the
// discriminated union: anything the union cannot match collapses to
// `{type:'unknown', raw}` so the feed builder never has to defend against a
// throw.

import { z } from 'zod';

const DispatchRunStatus = z.enum(['running', 'finished']);
type DispatchRunStatus = z.infer<typeof DispatchRunStatus>;

export const DispatchRunMeta = z.object({
  runId: z.string().regex(/^[A-Za-z0-9-]{1,100}$/),
  prompt: z.string(),
  agent: z.string().nullable().optional(),
  caller: z.string().nullable().optional(),
  claudeSessionId: z.string().nullable().optional(),
  model: z.string().nullable().optional(),
  startedAt: z.string(),
  finishedAt: z.string().nullable(),
  status: DispatchRunStatus,
  ok: z.boolean().nullable(),
  tokens: z
    .object({
      total: z.number(),
      input: z.number(),
      output: z.number(),
    })
    .nullable(),
  cost: z.number().nullable(),
  durationMs: z.number().nullable(),
});
export type DispatchRunMeta = z.infer<typeof DispatchRunMeta>;

const dispatchRunLineText = z.object({
  type: z.literal('text'),
  text: z.string(),
});
const dispatchRunLineReasoning = z.object({
  type: z.literal('reasoning'),
  text: z.string(),
});
const dispatchRunLineToolUse = z.object({
  type: z.literal('tool_use'),
  name: z.string(),
  input: z.unknown().optional(),
  output: z.unknown().optional(),
});
const dispatchRunLineStepStart = z.object({
  type: z.literal('step_start'),
});
const dispatchRunLineStepFinish = z.object({
  type: z.literal('step_finish'),
});
const dispatchRunLineError = z.object({
  type: z.literal('error'),
  message: z.string().optional(),
});
const dispatchRunLineUnknown = z.object({
  type: z.literal('unknown'),
  raw: z.string(),
});

export const DispatchRunLine = z.discriminatedUnion('type', [
  dispatchRunLineText,
  dispatchRunLineReasoning,
  dispatchRunLineToolUse,
  dispatchRunLineStepStart,
  dispatchRunLineStepFinish,
  dispatchRunLineError,
  dispatchRunLineUnknown,
]);
export type DispatchRunLine = z.infer<typeof DispatchRunLine>;

export const DispatchRunDetail = DispatchRunMeta.extend({
  lines: z.array(z.string()),
});
export type DispatchRunDetail = z.infer<typeof DispatchRunDetail>;

export const DispatchRunsListResponse = z.object({
  runs: z.array(DispatchRunMeta),
});
export type DispatchRunsListResponse = z.infer<typeof DispatchRunsListResponse>;

export function parseDispatchRunLine(raw: string): DispatchRunLine {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
    // Deliberate silent swallow, not a missed log: this is a pure contracts
    // package (zod only, no logger dependency) so every caller can parse a
    // run line without carrying a logging dependency transitively.
    // eslint-disable-next-line mobile-learn/no-unhandled-catch -- see comment above; caller renders the unknown line verbatim
  } catch {
    return { type: 'unknown', raw };
  }
  const result = DispatchRunLine.safeParse(parsed);
  if (result.success) {
    return result.data;
  }
  return { type: 'unknown', raw };
}
