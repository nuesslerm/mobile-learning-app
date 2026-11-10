import { describe, expect, it } from 'vitest';

import { editSourceSchema } from '../src/contracts/editStats';
import { applyCommands } from '../src/use-cases/executor';
import {
  buildPseudoNote,
  buildRealizeRequest,
  parsePseudoCommand,
  pseudoLines,
  restrictAlternativeToLines,
} from '../src/use-cases/pseudoCode';

const STUB =
  'class Solution:\n    def isPalindrome(self, s: str) -> bool:\n        ';
const ONE_NOTE =
  'class Solution:\n    def isPalindrome(self, s: str) -> bool:\n        # keep only alphanumeric characters in lowercase';
const TWO_NOTES = `${ONE_NOTE}\n        # two pointers from both ends`;

describe('parsePseudoCommand: mode switches', () => {
  it('recognises enter and exit however whisper spells them', () => {
    // Given the mode words in a few spoken forms
    // When parsed outside pseudo mode
    // Then enter/exit are recognised, whisper's "sudo" included
    expect(
      parsePseudoCommand('enter pseudo code mode', STUB, 'python3', {
        pseudoMode: false,
      })
    ).toEqual({ kind: 'enter' });
    expect(
      parsePseudoCommand('switch to sudo code', STUB, 'python3', {
        pseudoMode: false,
      })
    ).toEqual({ kind: 'enter' });
    expect(
      parsePseudoCommand('exit pseudo code mode', STUB, 'python3', {
        pseudoMode: true,
      })
    ).toEqual({ kind: 'exit' });
    expect(
      parsePseudoCommand('pseudo code mode off', STUB, 'python3', {
        pseudoMode: true,
      })
    ).toEqual({ kind: 'exit' });
  });

  it('returns null for an ordinary edit outside pseudo mode', () => {
    // Given a normal instruction
    // When parsed with the mode off
    // Then it goes to the planner untouched
    expect(
      parsePseudoCommand(
        'set s to only its alphanumeric characters',
        STUB,
        'python3',
        { pseudoMode: false }
      )
    ).toBeNull();
  });
});

describe('buildPseudoNote', () => {
  it('replaces the whitespace-only stub body with the first note, keeping the stub indentation', () => {
    // Given the starter code with an empty body line
    // When the first note is spoken in pseudo mode
    const cmd = parsePseudoCommand(
      'keep only alphanumeric characters in lowercase.',
      STUB,
      'python3',
      { pseudoMode: true }
    );
    // Then the stub line becomes the comment
    expect(cmd?.kind).toBe('note');
    const after = applyCommands(
      STUB,
      (cmd as { alternative: { commands: never[] } }).alternative.commands
    ).text;
    expect(after).toBe(ONE_NOTE);
  });

  it('appends the next note at the same indentation as the previous one', () => {
    // Given one note in the body
    // When a second is spoken
    const note = buildPseudoNote(
      ONE_NOTE,
      'two pointers from both ends',
      'python3'
    );
    // Then it lands on its own line under the first, same indent
    expect(applyCommands(ONE_NOTE, note.commands).text).toBe(TWO_NOTES);
    expect(note.valid).toBe(true);
    expect(note.id).toBe('local-pseudo-note');
  });

  it('indents one level deeper after a line that opens a block, and uses // for brace languages', () => {
    // Given a JS function whose last line opens a block
    const js = 'function f(a) {\n    if (a) {';
    // When a note is added
    const note = buildPseudoNote(js, 'return early', 'javascript');
    // Then it is nested under the block with the language comment marker
    expect(applyCommands(js, note.commands).text).toBe(
      'function f(a) {\n    if (a) {\n        // return early'
    );
  });
});

describe('realise', () => {
  it('lists comment lines and builds a fenced request for one named line', () => {
    // Given two pseudo lines
    expect(pseudoLines(TWO_NOTES, 'python3')).toEqual([3, 4]);
    // When line four is named, as a word
    const cmd = parsePseudoCommand(
      'implement the pseudo code on line four',
      TWO_NOTES,
      'python3',
      { pseudoMode: false }
    );
    // Then the request targets exactly that line and says so to the planner
    expect(cmd?.kind).toBe('realize');
    const request = (
      cmd as {
        request: { startLine: number; endLine: number; transcript: string };
      }
    ).request;
    expect(request.startLine).toBe(4);
    expect(request.endLine).toBe(4);
    expect(request.transcript).toContain('line 4');
    expect(request.transcript).toContain('two pointers from both ends');
  });

  it('"all the pseudo code" spans the first to the last comment line', () => {
    // Given two pseudo lines
    // When everything is asked for
    const request = buildRealizeRequest(TWO_NOTES, 'python3', 'all');
    // Then the range covers both
    expect(request).toMatchObject({ startLine: 3, endLine: 4 });
    expect(
      parsePseudoCommand(
        'turn all the pseudo code into real code',
        TWO_NOTES,
        'python3',
        { pseudoMode: true }
      )?.kind
    ).toBe('realize');
  });

  it('a realise that names a code line, not a comment, is not a realise', () => {
    // Given line 2 is the def line
    // When asked to implement line 2
    // Then outside pseudo mode it falls back to the planner (null); inside it is a note
    expect(
      parsePseudoCommand('implement line 2', TWO_NOTES, 'python3', {
        pseudoMode: false,
      })
    ).toBeNull();
    expect(
      parsePseudoCommand('implement line 2', TWO_NOTES, 'python3', {
        pseudoMode: true,
      })?.kind
    ).toBe('note');
  });

  it('restrictAlternativeToLines keeps an edit inside the line and rejects one that spills', () => {
    // Given an alternative that replaces exactly line 4 and one that also rewrites line 3
    const inside = {
      id: 'a',
      label: 'l',
      summary: 's',
      confidence: 0.9,
      commands: [
        {
          type: 'replaceLines' as const,
          startLine: 4,
          endLine: 4,
          newText: '        left, right = 0, len(s) - 1',
        },
      ],
    };
    const spill = {
      ...inside,
      commands: [
        {
          type: 'replaceLines' as const,
          startLine: 3,
          endLine: 4,
          newText: 'x',
        },
      ],
    };
    const rename = {
      ...inside,
      commands: [{ type: 'renameSymbol' as const, position: 0, newName: 'y' }],
    };
    // When fenced to line 4
    // Then only the first survives
    expect(
      restrictAlternativeToLines(inside, TWO_NOTES, {
        startLine: 4,
        endLine: 4,
      }).valid
    ).not.toBe(false);
    expect(
      restrictAlternativeToLines(spill, TWO_NOTES, { startLine: 4, endLine: 4 })
    ).toMatchObject({
      valid: false,
      invalidReason: 'Edit reaches outside line 4.',
    });
    expect(
      restrictAlternativeToLines(rename, TWO_NOTES, {
        startLine: 4,
        endLine: 4,
      }).valid
    ).toBe(false);
  });

  it('allows an offset edit that takes the newline before or after the fenced line', () => {
    // Given a v1-style replaceRange that starts on the line break ending line 3 and runs to the end of line 4
    const nl = TWO_NOTES.lastIndexOf('\n');
    const alt = {
      id: 'a',
      label: 'l',
      summary: 's',
      confidence: 0.9,
      commands: [
        {
          type: 'replaceRange' as const,
          start: nl,
          end: TWO_NOTES.length,
          newText: '\n        left, right = 0, len(s) - 1',
        },
      ],
    };
    // When fenced to line 4
    // Then it is still inside the fence; a span starting well inside line 3 is not
    expect(
      restrictAlternativeToLines(alt, TWO_NOTES, { startLine: 4, endLine: 4 })
        .valid
    ).not.toBe(false);
    const intoLineThree = {
      ...alt,
      commands: [{ ...alt.commands[0]!, start: nl - 6 }],
    };
    expect(
      restrictAlternativeToLines(intoLineThree, TWO_NOTES, {
        startLine: 4,
        endLine: 4,
      }).valid
    ).toBe(false);
  });

  it('judges an offset edit by its clamped span, as the executor applies it', () => {
    // Given the deployed v1 planner's actual answer: end one past the buffer, start one into the comment
    const alt = {
      id: 'a',
      label: 'l',
      summary: 's',
      confidence: 0.9,
      commands: [
        {
          type: 'replaceRange' as const,
          start: TWO_NOTES.lastIndexOf('#') + 1,
          end: TWO_NOTES.length + 1,
          newText: 'left, right = 0, len(s) - 1',
        },
      ],
    };
    // When fenced to the last line
    // Then it is inside the fence
    expect(
      restrictAlternativeToLines(alt, TWO_NOTES, { startLine: 4, endLine: 4 })
        .valid
    ).not.toBe(false);
  });

  it('normalises an in-fence offset answer to replaceLines over exactly the fenced line, restoring indentation', () => {
    // Given the deployed v1 planner's live answer: a span one character early at both ends, flush newText
    const nl = TWO_NOTES.lastIndexOf('\n');
    const alt = {
      id: 'a',
      label: 'l',
      summary: 's',
      confidence: 0.9,
      commands: [
        {
          type: 'replaceRange' as const,
          start: nl,
          end: TWO_NOTES.length - 1,
          newText: 'left, right = 0, len(s) - 1',
        },
      ],
    };
    // When fenced to line 4 and applied
    const fenced = restrictAlternativeToLines(alt, TWO_NOTES, {
      startLine: 4,
      endLine: 4,
    });
    // Then exactly line 4 is replaced, indented like the comment it replaces, and nothing is glued to line 3
    expect(fenced.commands).toEqual([
      {
        type: 'replaceLines',
        startLine: 4,
        endLine: 4,
        newText: '        left, right = 0, len(s) - 1',
      },
    ]);
    expect(applyCommands(TWO_NOTES, fenced.commands).text).toBe(
      `${ONE_NOTE}\n        left, right = 0, len(s) - 1`
    );
  });

  it('treats a two-character overshoot as drift but a span reaching into the previous line as outside', () => {
    // Given the deployed planner's live answer for the last line: start two characters before the line break
    const SOLVED = `${TWO_NOTES.replace('# two pointers from both ends', 'right -= 1')}\n        # after the loop, return true`;
    const lineStart = SOLVED.lastIndexOf('\n') + 1;
    const drift = {
      id: 'a',
      label: 'l',
      summary: 's',
      confidence: 0.9,
      commands: [
        {
          type: 'replaceRange' as const,
          start: lineStart - 2,
          end: SOLVED.length - 2,
          newText: '        return True',
        },
      ],
    };
    // When fenced to that line
    const fenced = restrictAlternativeToLines(drift, SOLVED, {
      startLine: 5,
      endLine: 5,
    });
    // Then it is normalised to exactly that line and the previous line keeps its last character
    expect(fenced.valid).not.toBe(false);
    expect(applyCommands(SOLVED, fenced.commands).text).toBe(
      `${TWO_NOTES.replace('# two pointers from both ends', 'right -= 1')}\n        return True`
    );
    // And a span that starts well inside the previous line is refused
    const intent = {
      ...drift,
      commands: [{ ...drift.commands[0]!, start: lineStart - 8 }],
    };
    expect(
      restrictAlternativeToLines(intent, SOLVED, { startLine: 5, endLine: 5 })
        .valid
    ).toBe(false);
  });
});

describe('restrictAlternativeToLines: insertAfter mode', () => {
  it('passes an answer inserting after the anchor line and applies it as new lines below', () => {
    // Given an answer that inserts a new line after anchor line 4
    const insert = {
      id: 'a',
      label: 'l',
      summary: 's',
      confidence: 0.9,
      commands: [
        {
          type: 'insertAfterLine' as const,
          line: 4,
          text: 'return True',
        },
      ],
    };
    // When fenced to line 4 in insertAfter mode
    const fenced = restrictAlternativeToLines(
      insert,
      TWO_NOTES,
      { startLine: 4, endLine: 4 },
      { mode: 'insertAfter' }
    );
    // Then it is a single insertAfterLine at the anchor, indentation restored
    expect(fenced.valid).not.toBe(false);
    expect(fenced.commands).toEqual([
      { type: 'insertAfterLine', line: 4, text: '        return True' },
    ]);
    // And applying it adds the line below the anchor without changing it
    expect(applyCommands(TWO_NOTES, fenced.commands).text).toBe(
      `${TWO_NOTES}\n        return True`
    );
  });

  it('rejects an answer that rewrites the anchor line itself', () => {
    // Given an answer that replaces anchor line 4
    const rewrite = {
      id: 'a',
      label: 'l',
      summary: 's',
      confidence: 0.9,
      commands: [
        {
          type: 'replaceLines' as const,
          startLine: 4,
          endLine: 4,
          newText: '        right -= 1',
        },
      ],
    };
    // When fenced to line 4 in insertAfter mode
    // Then it is invalid
    expect(
      restrictAlternativeToLines(
        rewrite,
        TWO_NOTES,
        { startLine: 4, endLine: 4 },
        { mode: 'insertAfter' }
      ).valid
    ).toBe(false);
  });
});

describe('editSourceSchema', () => {
  it('parses the new plan sources alongside the existing ones', () => {
    // Given the plan-voice and plan-text sources
    // When parsed
    // Then they are accepted and unknown sources are not
    expect(editSourceSchema.parse('plan-voice')).toBe('plan-voice');
    expect(editSourceSchema.parse('plan-text')).toBe('plan-text');
    expect(editSourceSchema.parse('voice')).toBe('voice');
    expect(editSourceSchema.safeParse('plan').success).toBe(false);
  });
});
