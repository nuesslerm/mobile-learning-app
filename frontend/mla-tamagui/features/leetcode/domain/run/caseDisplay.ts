import type { TestResult } from './execution';

export type CaseRow = { index: number; result: TestResult };

/** The cases a person reads first: the ones that failed, then the ones that passed, each group in the order of the bundled cases. */
export function orderCases(results: readonly TestResult[]): CaseRow[] {
  const rows = results.map((result, index) => ({ index, result }));
  return [...rows.filter((row) => !row.result.pass), ...rows.filter((row) => row.result.pass)];
}

/** A value as one line of text: strings as they are, everything else as JSON. */
export function showValue(value: unknown): string {
  if (typeof value === 'string') return value;
  if (value === undefined) return 'None';
  return JSON.stringify(value) ?? String(value);
}

/** What fits on one line before a tap expands it. */
export const CLIP_LENGTH = 72;

/**
 * The first line that has text, cut after `limit` characters (whole characters: a cut between the halves of an emoji draws a broken
 * glyph). A trailing newline is not a second line: `print('x')` leaves `x\n`, and a value that fits is not offered for expanding.
 */
export function clip(text: string, limit = CLIP_LENGTH): { text: string; clipped: boolean } {
  const lines = text.trimEnd().split(/\r?\n/);
  const characters = Array.from(lines.find((line) => line.trim() !== '') ?? '');
  const clipped = lines.length > 1 || characters.length > limit;
  return { text: clipped ? `${characters.slice(0, limit).join('')}…` : characters.join(''), clipped };
}

/** The arguments of a case on one line, as `name = value` separated by commas. */
export function showInput(input: Record<string, unknown> | undefined): string {
  return Object.entries(input ?? {})
    .map(([name, value]) => `${name} = ${showValue(value)}`)
    .join(', ');
}
