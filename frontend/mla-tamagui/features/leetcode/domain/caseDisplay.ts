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

export function clip(text: string, limit = CLIP_LENGTH): { text: string; clipped: boolean } {
  const line = text.split('\n')[0] ?? '';
  const clipped = text.length > limit || line.length < text.length;
  return { text: clipped ? `${line.slice(0, limit)}…` : text, clipped };
}

/** The arguments of a case on one line, as `name = value` separated by commas. */
export function showInput(input: Record<string, unknown> | undefined): string {
  return Object.entries(input ?? {})
    .map(([name, value]) => `${name} = ${showValue(value)}`)
    .join(', ');
}
