import { z } from 'zod';

export const replaceRangeCommandSchema = z.object({
  type: z.literal('replaceRange'),
  start: z.number().int().min(0),
  end: z.number().int().min(0),
  newText: z.string(),
});

export const insertAtCommandSchema = z.object({
  type: z.literal('insertAt'),
  position: z.number().int().min(0),
  text: z.string(),
});

export const deleteRangeCommandSchema = z.object({
  type: z.literal('deleteRange'),
  start: z.number().int().min(0),
  end: z.number().int().min(0),
});

export const renameSymbolCommandSchema = z.object({
  type: z.literal('renameSymbol'),
  position: z.number().int().min(0),
  newName: z.string().min(1),
});

export const moveRangeCommandSchema = z.object({
  type: z.literal('moveRange'),
  start: z.number().int().min(0),
  end: z.number().int().min(0),
  insertAt: z.number().int().min(0),
});

export const wrapWithCommandSchema = z.object({
  type: z.literal('wrapWith'),
  start: z.number().int().min(0),
  end: z.number().int().min(0),
  before: z.string(),
  after: z.string(),
});

export const extractBlockCommandSchema = z.object({
  type: z.literal('extractBlock'),
  start: z.number().int().min(0),
  end: z.number().int().min(0),
  newName: z.string().min(1),
});

/**
 * Line-anchored commands. The planner names lines it can see (the user prompt
 * numbers them, 1-based); the client turns them into offsets. Offsets computed
 * by a language model were the largest source of broken edits: mid-token
 * splits, off-by-a-line inserts, a `pass` line replaced from its first
 * column. A line number is something the model reads, not computes.
 */
export const insertAfterLineCommandSchema = z.object({
  type: z.literal('insertAfterLine'),
  /** 1-based line the block goes after; 0 inserts before the first line. */
  line: z.number().int().min(0),
  /** The new lines, already indented for where they land; no trailing newline needed. */
  text: z.string().min(1),
});

export const replaceLinesCommandSchema = z.object({
  type: z.literal('replaceLines'),
  /** 1-based, inclusive. */
  startLine: z.number().int().min(1),
  endLine: z.number().int().min(1),
  /** Replacement for those whole lines, indented for where it lands; empty deletes them. */
  newText: z.string(),
});

export const deleteLinesCommandSchema = z.object({
  type: z.literal('deleteLines'),
  startLine: z.number().int().min(1),
  endLine: z.number().int().min(1),
});

export const editCommandSchema = z.discriminatedUnion('type', [
  insertAfterLineCommandSchema,
  replaceLinesCommandSchema,
  deleteLinesCommandSchema,
  replaceRangeCommandSchema,
  insertAtCommandSchema,
  deleteRangeCommandSchema,
  renameSymbolCommandSchema,
  moveRangeCommandSchema,
  wrapWithCommandSchema,
  extractBlockCommandSchema,
]);

export const editAlternativeSchema = z.object({
  id: z.string().min(1),
  label: z.string().min(1),
  summary: z.string().min(1),
  confidence: z.number().min(0).max(1),
  valid: z.boolean().optional(),
  invalidReason: z.string().optional(),
  commands: z.array(editCommandSchema),
});

export const voiceEditResponseSchema = z.object({
  transcript: z.string(),
  alternatives: z.array(editAlternativeSchema),
});

export type EditCommand = z.infer<typeof editCommandSchema>;
export type EditAlternative = z.infer<typeof editAlternativeSchema>;
export type VoiceEditResponse = z.infer<typeof voiceEditResponseSchema>;
