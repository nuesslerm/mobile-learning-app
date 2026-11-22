import { z } from 'zod';

export const SYMBOL_GROUP_IDS = ['g1', 'g2', 'g3', 'g4', 'g5', 'g6', 'g7', 'g8'] as const;
export type SymbolGroupId = (typeof SYMBOL_GROUP_IDS)[number];

export const SYMBOL_GROUP_MAX_CHARS = 4;

export type SymbolGroup = { id: SymbolGroupId; chars: string };

/** The characters of the recording's sheet, in the order its Reset gives. */
export const DEFAULT_SYMBOL_GROUPS: readonly SymbolGroup[] = [
  { id: 'g1', chars: '#$^%' },
  { id: 'g2', chars: '&|?!' },
  { id: 'g3', chars: '\\@~_' },
  { id: 'g4', chars: '+-*/' },
  { id: 'g5', chars: '\'"`=' },
  { id: 'g6', chars: '{}[]' },
  { id: 'g7', chars: '()<>' },
  { id: 'g8', chars: '.:,;' },
];

const INVISIBLE = /[\s\p{Cc}\p{Cf}]/gu;

/** Keeps what a key can type: no whitespace, control or invisible format characters, and at most four code points (an emoji counts as one). */
export function normalizeGroup(raw: string): string {
  return Array.from(raw.replace(INVISIBLE, '')).slice(0, SYMBOL_GROUP_MAX_CHARS).join('');
}

const symbolGroupSchema = z.object({
  id: z.enum(SYMBOL_GROUP_IDS),
  chars: z.string().refine((chars) => normalizeGroup(chars) === chars),
});

/** Exactly eight groups, one per id, each already normalised: a stored list that fails this is replaced as a whole. */
export const symbolGroupsSchema = z
  .array(symbolGroupSchema)
  .length(SYMBOL_GROUP_IDS.length)
  .refine((groups) => new Set(groups.map((group) => group.id)).size === groups.length);

/** The sheet names a row by its place, so a group that moves is renamed with the place. */
export function groupLabel(index: number): string {
  return `Group ${index + 1}`;
}

/** Empty groups are hidden from the rail. */
export function visibleGroups(groups: readonly SymbolGroup[]): SymbolGroup[] {
  return groups.filter((group) => group.chars.length > 0);
}

/** A group's characters as rows of at most two, the 2x2 block the rail draws. */
export function groupCells(chars: string): string[][] {
  const cells = Array.from(chars);
  const rows: string[][] = [];
  for (let at = 0; at < cells.length; at += 2) rows.push(cells.slice(at, at + 2));
  return rows;
}

/** The same list with the group at `from` placed at `to`; an index out of range or an unchanged place returns the list itself. */
export function moveGroup(groups: readonly SymbolGroup[], from: number, to: number): readonly SymbolGroup[] {
  if (from === to || from < 0 || to < 0 || from >= groups.length || to >= groups.length) return groups;
  const next = groups.slice();
  const [moved] = next.splice(from, 1);
  next.splice(to, 0, moved);
  return next;
}

export function setGroupChars(groups: readonly SymbolGroup[], id: SymbolGroupId, raw: string): readonly SymbolGroup[] {
  const chars = normalizeGroup(raw);
  return groups.map((group) => (group.id === id ? { ...group, chars } : group));
}

export function resetGroups(): readonly SymbolGroup[] {
  return DEFAULT_SYMBOL_GROUPS.map((group) => ({ ...group }));
}

export function isDefaultGroups(groups: readonly SymbolGroup[]): boolean {
  return groups.length === DEFAULT_SYMBOL_GROUPS.length && groups.every((group, index) => group.id === DEFAULT_SYMBOL_GROUPS[index].id && group.chars === DEFAULT_SYMBOL_GROUPS[index].chars);
}
