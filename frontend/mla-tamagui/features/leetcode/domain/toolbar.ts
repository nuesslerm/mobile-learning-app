/**
 * Toolbar (keyboard-bar) configuration domain model.
 *
 * Describes which keys the coding keyboard bar shows and in what order. A key is
 * either a built-in coding action (referenced by id) or a user-defined custom
 * key that inserts a string at the cursor. Pure: types + Zod schemas only, no
 * React / lucide / IO (icons + actions live in the code-editor layer).
 */

import { z } from 'zod';

export const builtinKeyIdSchema = z.enum([
  'Tab',
  'Dedent',
  'Paren',
  'Bracket',
  'Brace',
  'Quote',
  'Apostrophe',
  'Colon',
  'Equals',
  'Underscore',
  'Dot',
  'Comment',
  'Newline',
  'Left',
  'Right',
  'Undo',
  'Redo',
  'RenameSymbol',
]);
export type BuiltinKeyId = z.infer<typeof builtinKeyIdSchema>;

/** All built-in key ids, in their default order. */
export const BUILTIN_KEY_IDS = builtinKeyIdSchema.options;

/** Max characters for a custom key's on-bar glyph, so it stays uniform width. */
export const CUSTOM_KEY_LABEL_MAX = 3;

export const toolbarItemSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('builtin'), id: builtinKeyIdSchema }),
  z.object({
    kind: z.literal('custom'),
    id: z.string().min(1),
    label: z.string().min(1).max(CUSTOM_KEY_LABEL_MAX),
    insert: z.string().min(1),
    cursorOffset: z.number().int().optional(),
  }),
]);
export type ToolbarItem = z.infer<typeof toolbarItemSchema>;

export const toolbarConfigSchema = z.array(toolbarItemSchema);

/** Default toolbar: every built-in key, in declaration order. */
export const DEFAULT_TOOLBAR_ITEMS: ToolbarItem[] = BUILTIN_KEY_IDS.map(
  (id) => ({
    kind: 'builtin' as const,
    id,
  })
);

/** Stable identity for an item — used for React keys and removal. */
export function toolbarItemKey(item: ToolbarItem): string {
  return item.kind === 'builtin' ? `builtin:${item.id}` : `custom:${item.id}`;
}
