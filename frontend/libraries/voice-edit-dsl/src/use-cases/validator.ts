import type { EditCommand, EditAlternative } from '../contracts/schema';
import { clampCommand, commandSpan, snapCommand } from './executor';
import { resolveLineCommands, type OffsetCommand } from './lineCommands';

/**
 * How much one spoken instruction is allowed to change.
 *
 * Voice editing is worth having because it is incremental: say a step, see it
 * land, say the next one. "Write a two-pointer solution that..." is not that --
 * it is dictating a prompt to a code generator and reading the answer, and it
 * fails in the ways that implies. A whole function arrives in one block, so a
 * mistake anywhere in it can only be fixed by describing the whole thing
 * again; nothing was ever verified a step at a time; and the transcript that
 * produced it is too long for a mis-hearing to be obvious.
 *
 * So the size cap is a feature, not a safety valve. An instruction that would
 * add more than a handful of lines at once is refused with an explanation,
 * which pushes the session back toward "declare left and right", "add a while
 * loop over them", "skip non-alphanumeric characters" -- steps small enough to
 * check by eye and cheap enough to redo.
 */
export interface EditSizeLimits {
  /** Most new lines a single command may add. */
  maxInsertedLines: number;
  /** Most new lines one alternative may add across all its commands. */
  maxInsertedLinesTotal: number;
}

export const DEFAULT_EDIT_SIZE_LIMITS: EditSizeLimits = {
  maxInsertedLines: 6,
  maxInsertedLinesTotal: 10,
};

const WHOLE_SOLUTION_ASK = /\b(?:write|implement|code|solve|create|build)\b[^.?!]*\b(?:solution|algorithm)\b/i;

/**
 * "Write a two pointer solution" asks for everything in one breath. The planner is told to answer with a
 * first step only, but a model that does not leaves a compact solution (two sum is five lines) inside the
 * ordinary limits, so such an ask gets the size of a first step: declare the variables or the signature.
 */
export function editSizeLimitsFor(transcript: string | null | undefined): EditSizeLimits {
  return transcript && WHOLE_SOLUTION_ASK.test(transcript)
    ? { maxInsertedLines: 3, maxInsertedLinesTotal: 3 }
    : DEFAULT_EDIT_SIZE_LIMITS;
}

/** Lines a command's own inserted text spans (1 for a single-line insert). */
function insertedLineCount(command: OffsetCommand): number {
  const added =
    command.type === 'replaceRange'
      ? command.newText
      : command.type === 'insertAt'
        ? command.text
        : command.type === 'wrapWith'
          ? command.before + command.after
          : '';
  if (added === '') return 0;
  return added.split('\n').length;
}

export interface ValidationResult {
  valid: boolean;
  reason?: string;
  failedCommandIndex?: number;
}

/**
 * Every command's offsets refer to the ORIGINAL text -- see `applyCommands` --
 * so each one is checked against that text, not against the result of the ones
 * before it. Validating against progressively-rewritten text was how a
 * perfectly good second command could be reported as out of range.
 */
export function validateCommands(
  text: string,
  rawCommands: EditCommand[],
  limits: EditSizeLimits = DEFAULT_EDIT_SIZE_LIMITS
): ValidationResult {
  const commands = resolveLineCommands(text, rawCommands);
  const seen: Array<{ start: number; end: number }> = [];
  let insertedTotal = 0;

  for (let i = 0; i < commands.length; i++) {
    const clamped = snapCommand(text, clampCommand(text, commands[i]!));
    const result = validateCommand(text, clamped);
    if (!result.valid) {
      return { ...result, failedCommandIndex: i };
    }
    // Two commands over the same characters cannot both mean what the planner
    // intended, since both were written against the untouched text. Catching
    // it here is what keeps `applyCommands` from having to throw at the user.
    const span = commandSpan(clamped);
    const clash = seen.find(
      (other) => other.start < span.end && span.start < other.end
    );
    if (clash) {
      return {
        valid: false,
        reason: `command range [${span.start}, ${span.end}) overlaps an earlier command's [${clash.start}, ${clash.end}) -- both refer to the code before any command ran`,
        failedCommandIndex: i,
      };
    }
    const inserted = insertedLineCount(clamped);
    if (inserted > limits.maxInsertedLines) {
      return {
        valid: false,
        reason: `this step writes ${inserted} lines at once (limit ${limits.maxInsertedLines}) -- say it as smaller steps, e.g. declare the variables, then add the loop, then the condition`,
        failedCommandIndex: i,
      };
    }
    insertedTotal += inserted;
    if (insertedTotal > limits.maxInsertedLinesTotal) {
      return {
        valid: false,
        reason: `this step writes ${insertedTotal} lines in total (limit ${limits.maxInsertedLinesTotal}) -- say it as smaller steps`,
        failedCommandIndex: i,
      };
    }

    seen.push(span);
  }

  return { valid: true };
}

export function validateAlternative(
  alternative: EditAlternative,
  text: string,
  limits: EditSizeLimits = DEFAULT_EDIT_SIZE_LIMITS
): EditAlternative {
  const result = validateCommands(text, alternative.commands, limits);
  return {
    ...alternative,
    valid: result.valid,
    invalidReason: result.reason,
  };
}

function validateCommand(text: string, command: OffsetCommand): ValidationResult {
  switch (command.type) {
    case 'replaceRange':
      return checkRange(text, command.start, command.end, 'replaceRange');
    case 'insertAt':
      if (command.position > text.length) {
        return { valid: false, reason: `insertAt position ${command.position} exceeds text length ${text.length}` };
      }
      return { valid: true };
    case 'deleteRange':
      return checkRange(text, command.start, command.end, 'deleteRange');
    case 'renameSymbol': {
      if (command.position > text.length) {
        return { valid: false, reason: `renameSymbol position ${command.position} exceeds text length ${text.length}` };
      }
      // `snapCommand` has already pulled the position onto the nearest
      // identifier on its line; if there was none to find, the rename would
      // match the empty string and rewrite every word boundary in the file.
      if (!/[A-Za-z0-9_]/.test(text[command.position] ?? '')) {
        return {
          valid: false,
          reason: `renameSymbol position ${command.position} is not on an identifier`,
        };
      }
      return { valid: true };
    }
    case 'moveRange':
      return checkRange(text, command.start, command.end, 'moveRange');
    case 'wrapWith':
      return checkRange(text, command.start, command.end, 'wrapWith');
    case 'extractBlock':
      return checkRange(text, command.start, command.end, 'extractBlock');
  }
}

function checkRange(text: string, start: number, end: number, label: string): ValidationResult {
  if (start > end) return { valid: false, reason: `${label} start (${start}) > end (${end})` };
  if (end > text.length) return { valid: false, reason: `${label} end (${end}) exceeds text length (${text.length})` };
  if (start < 0 || end < 0) return { valid: false, reason: `${label} range contains negative index` };
  return { valid: true };
}
