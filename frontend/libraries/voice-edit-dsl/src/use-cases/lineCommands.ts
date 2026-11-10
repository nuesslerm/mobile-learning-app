import type { EditCommand } from '../contracts/schema';

export type LineCommand = Extract<EditCommand, { type: 'insertAfterLine' | 'replaceLines' | 'deleteLines' }>;
export type OffsetCommand = Exclude<EditCommand, LineCommand>;

export function isLineCommand(command: EditCommand): command is LineCommand {
  return command.type === 'insertAfterLine' || command.type === 'replaceLines' || command.type === 'deleteLines';
}

/** Offset of the first character of each 1-based line, plus text.length as the sentinel after the last line. */
export function lineStarts(text: string): number[] {
  const starts = [0];
  for (let i = 0; i < text.length; i++) if (text[i] === '\n') starts.push(i + 1);
  return starts;
}

function clampLine(line: number, lineCount: number): number {
  return Math.max(1, Math.min(line, lineCount));
}

/**
 * Turns a line-anchored command into the offset command the executor already
 * understands, against `text`. Out-of-range lines clamp to the buffer rather
 * than throw: the planner miscounting the last line by one must not cost the
 * speaker the turn, and the result is still an edit on a real line.
 *
 * - insertAfterLine n: a block on its own lines after line n (n = 0: before
 *   line 1). Lands at the end of line n and brings the newline itself, so an
 *   unterminated last line stays intact.
 * - replaceLines a..b: the whole lines, keeping the newline after b (or the
 *   lack of one, at end of file). Empty `newText` removes the lines entirely.
 * - deleteLines a..b: the lines and their newlines.
 */
export function resolveLineCommand(text: string, command: LineCommand): OffsetCommand {
  const starts = lineStarts(text);
  const lineCount = Math.max(1, starts.length - (text.endsWith('\n') ? 1 : 0));
  const endOfLine = (line: number): number => {
    // line is 1-based and within range
    const next = starts[line];
    return next === undefined ? text.length : next - 1;
  };
  switch (command.type) {
    case 'insertAfterLine': {
      const block = command.text.replace(/\n+$/, '');
      if (command.line <= 0) {
        return { type: 'insertAt', position: 0, text: `${block}\n` };
      }
      const line = clampLine(command.line, lineCount);
      const position = endOfLine(line);
      return { type: 'insertAt', position, text: `\n${block}` };
    }
    case 'replaceLines':
    case 'deleteLines': {
      const a = clampLine(Math.min(command.startLine, command.endLine), lineCount);
      const b = clampLine(Math.max(command.startLine, command.endLine), lineCount);
      const start = starts[a - 1] ?? 0;
      const end = endOfLine(b);
      if (command.type === 'deleteLines' || command.newText === '') {
        // Take the newline after the block too, so no blank line is left behind;
        // at end of file take the newline before it instead.
        if (end < text.length) return { type: 'deleteRange', start, end: end + 1 };
        return { type: 'deleteRange', start: Math.max(0, start - 1), end };
      }
      return { type: 'replaceRange', start, end, newText: command.newText.replace(/\n+$/, '') };
    }
  }
}

/** Every command as an offset command against `text`; offset commands pass through untouched. */
export function resolveLineCommands(text: string, commands: EditCommand[]): OffsetCommand[] {
  return commands.map((c) => (isLineCommand(c) ? resolveLineCommand(text, c) : c));
}

/** The code as the planner sees it: `N| line`, 1-based, so a named line is unambiguous. */
export function numberLines(code: string): string {
  const lines = code.split('\n');
  const width = String(lines.length).length;
  return lines.map((l, i) => `${String(i + 1).padStart(width, ' ')}| ${l}`).join('\n');
}
