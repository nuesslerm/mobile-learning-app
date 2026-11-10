import { z } from 'zod';

const replaceRangeSchema = z.object({
  tool: z.literal('replaceRange'),
  params: z.object({
    from: z.number().int().min(0),
    to: z.number().int().min(0),
    text: z.string().max(200),
  }),
});

const insertAtSchema = z.object({
  tool: z.literal('insertAt'),
  params: z.object({
    position: z.number().int().min(0),
    text: z.string().max(200),
  }),
});

const deleteRangeSchema = z.object({
  tool: z.literal('deleteRange'),
  params: z.object({
    from: z.number().int().min(0),
    to: z.number().int().min(0),
  }),
});

const renameSymbolSchema = z.object({
  tool: z.literal('renameSymbol'),
  params: z.object({
    oldName: z.string().min(1).max(100),
    newName: z.string().min(1).max(100),
  }),
});

export const toolCallSchema = z.discriminatedUnion('tool', [
  replaceRangeSchema,
  insertAtSchema,
  deleteRangeSchema,
  renameSymbolSchema,
]);

export type ToolCall = z.infer<typeof toolCallSchema>;
