import type { EditCommand } from '../contracts/schema';
import { resolveLineCommands, type OffsetCommand } from './lineCommands';

export interface ApplyResult {
  text: string;
  selection: { start: number; end: number };
}

/**
 * Every command in one alternative carries offsets into the code the planner
 * was GIVEN -- `prompts.ts` says "zero-based UTF-16 string offsets in the
 * current code", and the planner sees that text once, before any command runs.
 * So applying command 2 at its stated offsets against the text command 1
 * already changed reads the wrong characters, by exactly the length command 1
 * added or removed.
 *
 * That is not a subtle drift. Seen live on a two-command re-indent of a real
 * solution: the first edit ate a quote off a `"""` docstring opener and the
 * second replaced `l, r ` instead of the whitespace before it, leaving
 * `= 0, len(s) - 1` and code that does not parse. A voice feature cannot be
 * iterated on when each accepted edit may quietly corrupt the buffer.
 *
 * So each command's offsets are rebased by the net length change of the
 * commands already applied that sit entirely BEFORE it. Commands are applied in
 * their given order, which keeps the final cursor where the last command put
 * it, and overlapping commands are rejected rather than silently mangled --
 * two edits to the same span cannot both mean what the planner intended.
 */
export function applyCommands(text: string, rawCommands: EditCommand[]): ApplyResult {
  // Line-anchored commands become offset commands against the ORIGINAL text,
  // which is the same frame every offset command is already written in.
  const commands: OffsetCommand[] = resolveLineCommands(text, rawCommands);
  let current = text;
  let cursor = 0;
  const applied: Array<{ start: number; end: number; delta: number }> = [];

  for (const command of commands) {
    const span = commandSpan(command);
    let shift = 0;
    for (const previous of applied) {
      if (previous.end <= span.start) {
        shift += previous.delta;
      } else if (previous.start < span.end) {
        throw new Error(
          `applyCommands: command overlaps an earlier one (original [${span.start}, ${span.end}) vs [${previous.start}, ${previous.end})) -- both offsets refer to the code before any command ran, so they cannot be applied together`
        );
      }
    }

    const before = current.length;
    const result = applyCommand(
      current,
      snapCommand(current, clampCommand(current, shiftCommand(command, shift)))
    );
    applied.push({
      start: span.start,
      end: span.end,
      delta: result.text.length - before,
    });
    current = result.text;
    cursor = result.selection.end;
  }

  return { text: current, selection: { start: cursor, end: cursor } };
}

/**
 * Pulls a command's offsets inside the text it will be applied to.
 *
 * Planners overshoot the end of the buffer by a character routinely -- three
 * alternatives in one live run all carried `end: 69` against 68 characters of
 * code, and every one of them was rejected, leaving the speaker with nothing to
 * accept for an instruction the model had otherwise understood perfectly. An
 * `end` past the last character has only one possible meaning ("to the end"),
 * so honouring it beats refusing the whole edit. Genuinely contradictory
 * commands -- `start` after `end` -- are still rejected by the validator.
 */
export function clampCommand(text: string, command: OffsetCommand): OffsetCommand {
  const fit = (n: number): number => Math.max(0, Math.min(n, text.length));
  switch (command.type) {
    case 'insertAt':
    case 'renameSymbol':
      return { ...command, position: fit(command.position) };
    case 'moveRange':
      return {
        ...command,
        start: fit(command.start),
        end: fit(command.end),
        insertAt: fit(command.insertAt),
      };
    default:
      return { ...command, start: fit(command.start), end: fit(command.end) };
  }
}

/**
 * Moves a line-oriented edit onto a line boundary.
 *
 * Planners are unreliable at counting UTF-16 offsets, and the failure is ugly:
 * asked to add two inner while loops at the top of a loop body, one returned an
 * insert position three characters early and split the line above it, leaving
 * `while left < righ` ... `t:` -- code that cannot parse, from an instruction it
 * had understood. The app had sent the right buffer (`codeLength` 253 matched
 * the editor exactly); only the offset was wrong.
 *
 * An edit whose own text spans lines is a block edit, and a block edit never
 * means "split the middle of this token" -- so when such a boundary lands
 * inside a word, it snaps to the nearest line boundary. Edits that stay within
 * a line (a rename, a changed operator) are left exactly where they are: there
 * the middle of a line is the whole point.
 */
/**
 * A block whose first line is bare while its later lines are indented lost
 * its first line's indentation somewhere between the planner and here (the
 * planner counted the `pass` line's start as the start of `pass`, live, and
 * wrote `if ...:` in column 0 above a properly indented `return False`).
 * Give the first line the indentation of the line it lands on.
 */
function reindentBareFirstLine(text: string, lineStart: number, block: string): string {
  if (/^[ \t\n]/.test(block) || !/\n[ \t]+\S/.test(block)) return block;
  const indent = /^[ \t]*/.exec(text.slice(lineStart))?.[0] ?? '';
  return indent.length > 0 ? `${indent}${block}` : block;
}

function atLineBoundary(text: string, offset: number): boolean {
  if (offset <= 0 || offset >= text.length) return true;
  // A position is on a boundary if a newline sits on either side of it. The
  // first version of this asked whether BOTH neighbours were word characters,
  // which let `right -= |1` through untouched -- the space before the offset
  // made it look safe -- and `return True` landed between the operator and its
  // operand, leaving `right -=` and `return True1`.
  return text[offset - 1] === '\n' || text[offset] === '\n';
}

function lineStartOf(text: string, offset: number): number {
  return text.lastIndexOf('\n', offset - 1) + 1;
}

function lineEndOf(text: string, offset: number): number {
  const i = text.indexOf('\n', offset);
  return i === -1 ? text.length : i;
}

/** Nearest identifier character to `offset`, searched within its own line. */
function snapToIdentifier(text: string, offset: number): number {
  const isIdent = (ch: string | undefined): boolean =>
    ch !== undefined && /[A-Za-z0-9_]/.test(ch);
  if (isIdent(text[offset])) return offset;
  const lineStart = text.lastIndexOf('\n', offset - 1) + 1;
  const lineEndIndex = text.indexOf('\n', offset);
  const lineEnd = lineEndIndex === -1 ? text.length : lineEndIndex;
  for (let d = 1; d <= lineEnd - lineStart; d++) {
    if (offset - d >= lineStart && isIdent(text[offset - d])) return offset - d;
    if (offset + d < lineEnd && isIdent(text[offset + d])) return offset + d;
  }
  return offset;
}

export function snapCommand(text: string, command: OffsetCommand): OffsetCommand {
  switch (command.type) {
    case 'renameSymbol':
      // Same ±3 offset error the block edits get, with a far worse landing:
      // a position on whitespace makes `wordRangeAt` return an empty range,
      // and renaming the empty string rewrites every word boundary in the
      // file. Seen live -- "rename the variable right to high everywhere"
      // turned `class Solution:` into `highclasshigh highSolutionhigh:`.
      return { ...command, position: snapToIdentifier(text, command.position) };
    // Direction matters. An earlier version snapped to the NEAREST boundary,
    // and on "replace pass with an if ..." the planner's start sat on the `p`
    // of a `pass` at column 13: the line start was 13 away and the line end 4,
    // so start moved to the END of the line and the if-block was glued on
    // after the word it was meant to replace -- `passif s[left] != s[right]:`,
    // seen live. A block REPLACE means "these whole lines": start goes back to
    // its line start, end forward to its line end. A block INSERT that lands
    // mid-line goes after the statement it landed in, never before it.
    case 'insertAt': {
      if (!command.text.includes('\n')) return command;
      const lineStart = lineStartOf(text, command.position);
      const lineEnd = lineEndOf(text, command.position);
      if (lineEnd > lineStart && /^[ \t]*$/.test(text.slice(lineStart, lineEnd))) {
        // A whitespace-only line -- the starter's indented stub body. The
        // planner points into that indentation and writes its first line
        // bare, expecting the indentation to carry it: fill the line, and
        // do not start on a fresh one (that put `left = 0` in column 0).
        // A first line that brings its own indentation (insertAfterLine on
        // the stub, live) gives up as much as the line already has.
        const block = command.text.replace(/^\n/, '');
        const own = /^[ \t]*/.exec(block)?.[0].length ?? 0;
        return { ...command, position: lineEnd, text: block.slice(Math.min(own, lineEnd - lineStart)) };
      }
      const startsLine = command.position === 0 || text[command.position - 1] === '\n';
      if (startsLine) {
        // Already at the start of a line: a leading newline in the block only
        // opens a blank line, and a block without a trailing newline glues
        // the line it lands in front of onto its last line (`return True
        // right -= 1`, live).
        const trimmed = reindentBareFirstLine(text, command.position, command.text.replace(/^\n+/, ''));
        const followedByContent = command.position < text.length && text[command.position] !== '\n';
        const needsTrailing = followedByContent && !trimmed.endsWith('\n');
        return { ...command, text: needsTrailing ? `${trimmed}\n` : trimmed };
      }
      if (
        !atLineBoundary(text, command.position) &&
        /^[ \t]*$/.test(text.slice(lineStart, command.position))
      ) {
        // Only indentation before the position: the planner pointed at the
        // first token of a line ("before the while loop" planned as an insert
        // right before `while`). The block belongs above that line, not
        // below it -- snapping to the line end put a reassignment of `s`
        // inside the loop header, live.
        return {
          ...command,
          position: lineStart,
          text: command.text.endsWith('\n') ? command.text : `${command.text}\n`,
        };
      }
      const position = atLineBoundary(text, command.position)
        ? command.position
        : lineEndOf(text, command.position);
      // A block that lands at the END of a line (after `pass`, after
      // `right -= 1`) is only on its own line if it brings the newline with
      // it. Without this, "replace pass with an if ..." planned as an insert
      // at the end of the pass line came out as `passif s[left] != s[right]:`
      // -- twice, live, from a bundle that already had the boundary snap.
      const afterLineContent = position > 0 && text[position - 1] !== '\n';
      const needsNewline = afterLineContent && !command.text.startsWith('\n');
      return { ...command, position, text: needsNewline ? `\n${command.text}` : command.text };
    }
    case 'replaceRange': {
      if (!command.newText.includes('\n')) return command;
      const start = atLineBoundary(text, command.start) ? command.start : lineStartOf(text, command.start);
      const end = atLineBoundary(text, command.end) ? command.end : lineEndOf(text, command.end);
      // Snapping start outward can swallow the line's indentation. When it
      // does and the replacement's first line is bare, the planner was
      // counting on that indentation (it replaced the starter's indented stub
      // from one column in with `left = 0`): put it back.
      const swallowed = text.slice(start, command.start);
      const keepIndent = /^[ \t]+$/.test(swallowed) && !/^[ \t\n]/.test(command.newText);
      const newText = keepIndent
        ? `${swallowed}${command.newText}`
        : reindentBareFirstLine(text, start, command.newText);
      return { ...command, start, end, newText };
    }
    default:
      return command;
  }
}

/** The half-open span a command reads from, in ORIGINAL-text offsets. */
export function commandSpan(command: OffsetCommand): { start: number; end: number } {
  switch (command.type) {
    case 'insertAt':
    case 'renameSymbol':
      return { start: command.position, end: command.position };
    case 'moveRange':
      return {
        start: Math.min(command.start, command.insertAt),
        end: Math.max(command.end, command.insertAt),
      };
    default:
      return { start: command.start, end: command.end };
  }
}

/** The same command, with its offsets moved into current-text coordinates. */
function shiftCommand(command: OffsetCommand, shift: number): OffsetCommand {
  if (shift === 0) return command;
  switch (command.type) {
    case 'insertAt':
    case 'renameSymbol':
      return { ...command, position: command.position + shift };
    case 'moveRange':
      return {
        ...command,
        start: command.start + shift,
        end: command.end + shift,
        insertAt: command.insertAt + shift,
      };
    default:
      return { ...command, start: command.start + shift, end: command.end + shift };
  }
}

function applyCommand(text: string, command: OffsetCommand): ApplyResult {
  switch (command.type) {
    case 'replaceRange': {
      const replaced = text.slice(0, command.start) + command.newText + text.slice(command.end);
      return {
        text: replaced,
        selection: { start: command.start, end: command.start + command.newText.length },
      };
    }
    case 'insertAt': {
      const inserted = text.slice(0, command.position) + command.text + text.slice(command.position);
      return {
        text: inserted,
        selection: { start: command.position, end: command.position + command.text.length },
      };
    }
    case 'deleteRange': {
      const deleted = text.slice(0, command.start) + text.slice(command.end);
      return { text: deleted, selection: { start: command.start, end: command.start } };
    }
    case 'renameSymbol': {
      const { start, end } = wordRangeAt(text, command.position);
      const oldName = text.slice(start, end);
      if (oldName === '') {
        // `\b\b` matches at every word boundary, so renaming "" replaces the
        // gap between every token in the file. Refusing is the only safe
        // answer; `snapCommand` above is what stops it happening at all.
        throw new Error(
          `renameSymbol: position ${command.position} is not on an identifier`
        );
      }
      const escaped = oldName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      const renamed = text.replace(new RegExp(`\\b${escaped}\\b`, 'g'), command.newName);
      return {
        text: renamed,
        selection: { start, end: start + command.newName.length },
      };
    }
    case 'moveRange': {
      const movedText = text.slice(command.start, command.end);
      const without = text.slice(0, command.start) + text.slice(command.end);
      const insertAt = Math.min(command.insertAt, without.length);
      const moved = without.slice(0, insertAt) + movedText + without.slice(insertAt);
      return {
        text: moved,
        selection: { start: insertAt, end: insertAt + movedText.length },
      };
    }
    case 'wrapWith': {
      const wrapped =
        text.slice(0, command.start) +
        command.before +
        text.slice(command.start, command.end) +
        command.after +
        text.slice(command.end);
      return {
        text: wrapped,
        selection: { start: command.start, end: command.start + command.before.length + (command.end - command.start) + command.after.length },
      };
    }
    case 'extractBlock': {
      const block = text.slice(command.start, command.end);
      const replacement = command.newName + '();';
      const extracted = text.slice(0, command.start) + replacement + text.slice(command.end);
      const declaration = `\n\nfunction ${command.newName}() {\n${block}\n}`;
      return {
        text: extracted + declaration,
        selection: { start: command.start, end: command.start + replacement.length },
      };
    }
  }
}

function wordRangeAt(text: string, position: number): { start: number; end: number } {
  let start = position;
  while (start > 0 && /[A-Za-z0-9_]/.test(text[start - 1]!)) start--;
  let end = position;
  while (end < text.length && /[A-Za-z0-9_]/.test(text[end]!)) end++;
  return { start, end };
}
