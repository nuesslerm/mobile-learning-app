import { UNPRINTABLE } from './values';

export type Located = { line: number; column?: number };

const FRAME = /^:(\d+):(\d+)\)?\s*$/;

function ownLocation(line: number, column: number, firstLineOffset: number): Located | null {
  if (line === 1) {
    // line 1 carries the harness before the learner's first character: a position inside it is the harness's own
    if (column <= firstLineOffset) return null;
    return { line, column: column - firstLineOffset };
  }
  return { line, column };
}

/**
 * The innermost stack frame that names the script itself, in the learner's own code. V8 prints `at name (url:line:column)` and
 * JavaScriptCore `name@url:line:column`; a frame of the harness (line 1, before the learner's text) is skipped.
 */
export function locationFromStack(stack: unknown, scriptName: string, firstLineOffset: number): Located | null {
  if (typeof stack !== 'string' || scriptName === '') return null;
  for (const text of stack.split('\n')) {
    const at = text.lastIndexOf(scriptName);
    if (at < 0) continue;
    const found = FRAME.exec(text.slice(at + scriptName.length));
    if (!found) continue;
    const located = ownLocation(Number(found[1]), Number(found[2]), firstLineOffset);
    if (located) return located;
  }
  return null;
}

/** The position an ErrorEvent reports for a syntax error or a module-level throw (`colno` 0 means the engine gave none). */
export function locationFromEvent(event: { lineno?: number; colno?: number }, firstLineOffset: number): Located | null {
  const { lineno, colno } = event;
  if (typeof lineno !== 'number' || !Number.isInteger(lineno) || lineno < 1) return null;
  if (typeof colno !== 'number' || !Number.isInteger(colno) || colno < 1) return { line: lineno };
  return ownLocation(lineno, colno, firstLineOffset) ?? { line: lineno };
}

/** `TypeError: message` for an error, the text of anything else that was thrown, and <unprintable> when printing it throws. */
export function describeError(error: unknown): string {
  try {
    if (error instanceof Error) return `${error.name}: ${error.message}`;
    return String(error);
  } catch {
    return UNPRINTABLE;
  }
}
