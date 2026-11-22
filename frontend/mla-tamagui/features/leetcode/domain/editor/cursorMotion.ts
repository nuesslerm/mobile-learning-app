/**
 * Pure caret steps over a text: characters, words and lines, plus the line and column of a position.
 *
 * Positions are UTF-16 offsets, as the editor API counts them, and a step never lands inside a surrogate pair: characters are code
 * points, not grapheme clusters. Lines are logical lines (a wrapped line is one line). The text is the editor's own, read by the caller.
 */

export type Motion = { readonly unit: 'char' | 'word' | 'line'; readonly count: number };

export type LineColumn = { readonly line: number; readonly column: number };

type CharClass = 'space' | 'newline' | 'word' | 'punct';

const WORD = /^[\p{L}\p{N}_]$/u;

function classOf(char: string): CharClass {
  if (char === '\n') return 'newline';
  if (char === ' ' || char === '\t' || char === '\r') return 'space';
  return WORD.test(char) ? 'word' : 'punct';
}

const clamp = (value: number, min: number, max: number): number => Math.max(min, Math.min(max, value));

const isHigh = (unit: number): boolean => unit >= 0xd800 && unit <= 0xdbff;
const isLow = (unit: number): boolean => unit >= 0xdc00 && unit <= 0xdfff;

/** The position at the start of the code point that holds `pos`, so a position inside a pair is held before it. */
function snap(text: string, pos: number): number {
  const at = clamp(pos, 0, text.length);
  return at > 0 && at < text.length && isLow(text.charCodeAt(at)) && isHigh(text.charCodeAt(at - 1)) ? at - 1 : at;
}

function charAt(text: string, pos: number): string {
  const code = text.codePointAt(pos);
  return code === undefined ? '' : String.fromCodePoint(code);
}

function charBefore(text: string, pos: number): string {
  if (pos <= 0) return '';
  const low = text.charCodeAt(pos - 1);
  return isLow(low) && pos >= 2 && isHigh(text.charCodeAt(pos - 2)) ? text.slice(pos - 2, pos) : text[pos - 1] ?? '';
}

function nextChar(text: string, pos: number): number {
  return Math.min(text.length, pos + Math.max(1, charAt(text, pos).length));
}

function prevChar(text: string, pos: number): number {
  return Math.max(0, pos - Math.max(1, charBefore(text, pos).length));
}

/** `count` characters from `pos` (negative is left), clamped to the text. */
export function stepChars(text: string, pos: number, count: number): number {
  let at = snap(text, pos);
  for (let left = Math.abs(Math.trunc(count)); left > 0; left -= 1) {
    const next = count > 0 ? nextChar(text, at) : prevChar(text, at);
    if (next === at) break;
    at = next;
  }
  return at;
}

/** A stop is the start of a word or of a run of punctuation, and the end of each line (the break itself is one stop). */
function isStop(text: string, pos: number): boolean {
  if (pos >= text.length) return true;
  const here = classOf(charAt(text, pos));
  if (here === 'newline') return true;
  if (here === 'space') return false;
  return pos === 0 || classOf(charBefore(text, pos)) !== here;
}

function nextStop(text: string, pos: number): number {
  let at = nextChar(text, snap(text, pos));
  while (at < text.length && !isStop(text, at)) at = nextChar(text, at);
  return at;
}

function prevStop(text: string, pos: number): number {
  let at = prevChar(text, snap(text, pos));
  while (at > 0 && !isStop(text, at)) at = prevChar(text, at);
  return at;
}

/** `count` stops from `pos` (negative is left): word starts, indentation skipped, each line break one stop, clamped to the text. */
export function stepWords(text: string, pos: number, count: number): number {
  let at = snap(text, pos);
  for (let left = Math.abs(Math.trunc(count)); left > 0; left -= 1) {
    const next = count > 0 ? nextStop(text, at) : prevStop(text, at);
    if (next === at) break;
    at = next;
  }
  return at;
}

function lineStart(text: string, pos: number): number {
  return pos <= 0 ? 0 : text.lastIndexOf('\n', pos - 1) + 1;
}

function lineEnd(text: string, pos: number): number {
  const next = text.indexOf('\n', pos);
  return next === -1 ? text.length : next;
}

/** The column of `pos` as an offset from its line's start, the unit `stepLines` keeps as its goal. */
export function columnOffset(text: string, pos: number): number {
  const at = clamp(pos, 0, text.length);
  return at - lineStart(text, at);
}

function lineIndexOf(text: string, pos: number): number {
  let index = 0;
  for (let at = text.indexOf('\n'); at !== -1 && at < pos; at = text.indexOf('\n', at + 1)) index += 1;
  return index;
}

function lineCount(text: string): number {
  return lineIndexOf(text, text.length + 1) + 1;
}

function startOfLine(text: string, index: number): number {
  let at = 0;
  for (let line = 0; line < index; line += 1) at = text.indexOf('\n', at) + 1;
  return at;
}

/**
 * `count` lines from `pos` (negative is up) at the goal column, which is the column of `pos` unless the caller keeps the column it
 * grabbed at; a shorter line takes its own end. Up from the first line goes to 0 and down from the last line to the text's end.
 */
export function stepLines(text: string, pos: number, count: number, goalColumn?: number): number {
  const at = snap(text, pos);
  const goal = Math.max(0, goalColumn ?? columnOffset(text, at));
  const target = lineIndexOf(text, at) + Math.trunc(count);
  if (target < 0) return 0;
  if (target >= lineCount(text)) return text.length;
  const start = startOfLine(text, target);
  return snap(text, Math.min(start + goal, lineEnd(text, start)));
}

/** Where `motion` takes `pos`; `goalColumn` only matters to line motions. */
export function moveBy(text: string, pos: number, motion: Motion, goalColumn?: number): number {
  if (motion.unit === 'char') return stepChars(text, pos, motion.count);
  if (motion.unit === 'word') return stepWords(text, pos, motion.count);
  return stepLines(text, pos, motion.count, goalColumn);
}

/** The line and column a person reads, both from 1, the column counted in characters. */
export function lineColumn(text: string, pos: number): LineColumn {
  const at = snap(text, pos);
  const start = lineStart(text, at);
  return { line: lineIndexOf(text, at) + 1, column: Array.from(text.slice(start, at)).length + 1 };
}
