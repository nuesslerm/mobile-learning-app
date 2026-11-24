import type { Blind75Problem } from '../models';

/** The arguments a problem takes, in order: the first bundled case's keys (none for a problem without cases). */
export function parameterNames(problem: Blind75Problem): string[] {
  return Object.keys(problem.testCases?.[0]?.inputs ?? {});
}

const CURLY_DOUBLE = /[“”]/g;
const CURLY_SINGLE = /[‘’]/g;

/** One JSON value as a phone keyboard may have typed it: curly quotes read as straight ones. */
export function parseJsonValue(text: string): { ok: true; value: unknown } | { ok: false } {
  try {
    return { ok: true, value: JSON.parse(text.replace(CURLY_DOUBLE, '"').replace(CURLY_SINGLE, "'")) };
  } catch {
    return { ok: false };
  }
}
