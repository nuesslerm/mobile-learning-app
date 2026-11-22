/**
 * Pure text-editing transforms for the coding keyboard bar.
 *
 * Each helper takes the current document text + selection and returns the new
 * text and the selection to apply afterwards. Keeping this logic pure (no React,
 * no WebView) makes the keyboard-bar actions trivial to reason about and unit
 * test; the bar just dispatches the result to the editor.
 *
 * All offsets are absolute character indices into `text`, matching the
 * CodeMirror selection model used across the bridge (see ./types `Selection`).
 */

import { PYTHON_PROFILE, type LanguageProfile } from './languageProfile';
import type { Selection } from './types';

/** One indentation level. Matches the editor's default tabSize (4). */
export const INDENT_UNIT = '    ';

export type EditResult = { text: string; selection: Selection };

type LineMapper = (line: string) => string;

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

/** Index of the first character of the line containing `pos`. */
function lineStartIndex(text: string, pos: number): number {
  return text.lastIndexOf('\n', pos - 1) + 1;
}

/** Index just past the last character of the line containing `pos`. */
function lineEndIndex(text: string, pos: number): number {
  const next = text.indexOf('\n', pos);
  return next === -1 ? text.length : next;
}

/**
 * Apply `map` to every line spanned by the selection.
 *
 * For an empty selection the caret is preserved (shifted by however much its own
 * line grew/shrank before it); for a range the whole transformed block stays
 * selected so repeated taps keep operating on the same lines.
 */
function mapSelectedLines(
  text: string,
  sel: Selection,
  map: LineMapper
): EditResult {
  const blockStart = lineStartIndex(text, Math.min(sel.start, sel.end));
  const blockEnd = lineEndIndex(text, Math.max(sel.start, sel.end));
  const block = text.slice(blockStart, blockEnd);
  const newBlock = block.split('\n').map(map).join('\n');
  const newText = text.slice(0, blockStart) + newBlock + text.slice(blockEnd);

  if (sel.start === sel.end) {
    const firstOld = block.split('\n')[0] ?? '';
    const firstNew = newBlock.split('\n')[0] ?? '';
    const lineDelta = firstNew.length - firstOld.length;
    const caret = clamp(sel.start + lineDelta, blockStart, newText.length);
    return { text: newText, selection: { start: caret, end: caret } };
  }

  const endDelta = newBlock.length - block.length;
  return {
    text: newText,
    selection: {
      start: blockStart,
      end: clamp(blockEnd + endDelta, blockStart, newText.length),
    },
  };
}

/** Insert literal text at the caret, replacing any selected range. */
function insertText(
  text: string,
  sel: Selection,
  insert: string
): EditResult {
  const from = Math.min(sel.start, sel.end);
  const to = Math.max(sel.start, sel.end);
  const newText = text.slice(0, from) + insert + text.slice(to);
  const caret = from + insert.length;
  return { text: newText, selection: { start: caret, end: caret } };
}

/** Indent: prepend one level (`unit`, four spaces by default) to every selected line, or insert it at the caret. */
export function indent(text: string, sel: Selection, unit: string = INDENT_UNIT): EditResult {
  if (sel.start === sel.end) {
    return insertText(text, sel, unit);
  }
  return mapSelectedLines(text, sel, (line) => unit + line);
}

/** Dedent: remove up to one indent level (`unit`) of leading spaces from each line. */
export function dedent(text: string, sel: Selection, unit: string = INDENT_UNIT): EditResult {
  return mapSelectedLines(text, sel, (line) => {
    let removed = 0;
    while (removed < unit.length && line[removed] === ' ') {
      removed += 1;
    }
    return line.slice(removed);
  });
}

const escapeRegExp = (value: string) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** Toggle line comments (the language's marker and a space) across the selected lines. */
export function toggleComment(
  text: string,
  sel: Selection,
  profile: LanguageProfile = PYTHON_PROFILE
): EditResult {
  const marker = escapeRegExp(profile.lineComment);
  const commented = new RegExp(`^\\s*${marker}`);
  const removal = new RegExp(`^(\\s*)${marker}\\s?`);
  const blockStart = lineStartIndex(text, Math.min(sel.start, sel.end));
  const blockEnd = lineEndIndex(text, Math.max(sel.start, sel.end));
  const lines = text.slice(blockStart, blockEnd).split('\n');
  const allCommented = lines.every(
    (line) => line.trim() === '' || commented.test(line)
  );
  const map: LineMapper = allCommented
    ? (line) => line.replace(removal, '$1')
    : (line) =>
        line.trim() === ''
          ? line
          : line.replace(/^(\s*)/, `$1${profile.lineComment} `);
  return mapSelectedLines(text, sel, map);
}

/**
 * Newline that keeps the current line's indentation, adding one extra level
 * after a line the language opens a block with (`:` for Python, `{` for the brace languages),
 * and, with the caret between a bracket and its closer, putting the closer on its own line.
 */
export function smartNewline(
  text: string,
  sel: Selection,
  profile: LanguageProfile = PYTHON_PROFILE,
  unit: string = INDENT_UNIT
): EditResult {
  const start = Math.min(sel.start, sel.end);
  const ls = lineStartIndex(text, start);
  const lineSoFar = text.slice(ls, start);
  const leading = /^[ \t]*/.exec(lineSoFar)?.[0] ?? '';
  const fullLine = text.slice(ls, lineEndIndex(text, start)).trimEnd();
  const opensBlock =
    profile.openers.some((opener) => fullLine.endsWith(opener)) ||
    (profile.statementOpener?.test(fullLine) ?? false);
  const nextIndent = opensBlock ? leading + unit : leading;
  const between = text.slice(start - 1, start + 1);
  if (sel.start === sel.end && start > 0 && profile.pairs.includes(between)) {
    const inner = `\n${leading + unit}`;
    const result = insertText(text, sel, `${inner}\n${leading}`);
    const caret = start + inner.length;
    return { text: result.text, selection: { start: caret, end: caret } };
  }
  return insertText(text, sel, `\n${nextIndent}`);
}

/** Collapse/move the caret one character left. */
export function moveLeft(text: string, sel: Selection): Selection {
  const pos =
    sel.start === sel.end
      ? clamp(sel.start - 1, 0, text.length)
      : Math.min(sel.start, sel.end);
  return { start: pos, end: pos };
}

/** Collapse/move the caret one character right. */
export function moveRight(text: string, sel: Selection): Selection {
  const pos =
    sel.start === sel.end
      ? clamp(sel.start + 1, 0, text.length)
      : Math.max(sel.start, sel.end);
  return { start: pos, end: pos };
}
