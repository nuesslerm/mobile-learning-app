import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  applyCommands,
  editSizeLimitsFor,
  parseLocalEdit,
  validateAlternative,
  voiceEditResponseSchema,
  type VoiceEditResponse,
} from '@lib/voice-edit-dsl';
import { describe, expect, it } from 'vitest';

import { ProblemCatalog } from '../../problemCatalog';

const STUBS = join(__dirname, '../../../testing/voiceStubs');

type Turn = { n: number; raw: unknown };

function stubbedProblems(): { id: string; turns: Turn[] }[] {
  return readdirSync(STUBS, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => {
      const turns = readdirSync(join(STUBS, entry.name))
        .filter((file) => file.endsWith('.plan.json'))
        .map((file) => ({ n: Number.parseInt(file, 10), raw: JSON.parse(readFileSync(join(STUBS, entry.name, file), 'utf8')) as unknown }))
        .sort((a, b) => a.n - b.n);
      return { id: entry.name, turns };
    });
}

const problems = stubbedProblems();

/** The code before each turn, from the starter, as the app would hold it when the person speaks the turns in order. */
function replay(id: string, turns: Turn[]): { n: number; response: VoiceEditResponse; before: string }[] {
  let text = ProblemCatalog.getByIdOrThrow(id).starterCode.python3 ?? '';
  return turns.map(({ n, raw }) => {
    const response = voiceEditResponseSchema.parse(raw);
    const before = text;
    text = applyCommands(text, response.alternatives[0]?.commands ?? []).text;
    return { n, response, before };
  });
}

describe('the voice-edit plan answers the e2e serves from its network stub', () => {
  it('given every committed plan answer, when it is parsed with the wire schema, then it is a valid response with a non-empty command list', () => {
    // Given the committed answers of every stubbed problem
    const answers = problems.flatMap(({ id, turns }) => turns.map(({ n, raw }) => ({ where: `${id}/${n}`, raw })));
    expect(answers.length).toBeGreaterThan(0);

    for (const { where, raw } of answers) {
      // When each is parsed
      const parsed = voiceEditResponseSchema.safeParse(raw);

      // Then it is a wire response with one alternative that has commands
      expect(parsed.success, where).toBe(true);
      const response = parsed.data as VoiceEditResponse;
      expect(response.alternatives, where).toHaveLength(1);
      expect(response.alternatives[0]?.commands.length, where).toBeGreaterThan(0);
    }
  });

  it.each(problems.map((p) => [p.id, p.turns] as const))(
    'given the plan answers for %s, when applied in order from its starter, then no turn throws and each alternative is valid for what was said',
    (id, turns) => {
      // Given the answers numbered from 1 without a gap, as the script asks for them by turn number
      expect(turns.map((t) => t.n)).toEqual(turns.map((_, i) => i + 1));

      // When they are applied in order from the catalog starter
      for (const { n, response, before } of replay(id, turns)) {
        const alternative = response.alternatives[0]!;
        const checked = validateAlternative(alternative, before, editSizeLimitsFor(response.transcript));

        // Then the app's own check accepts each one (so the screen applies it on its own) and each changes the code
        expect(checked.valid, `turn ${n}: ${checked.invalidReason}`).toBe(true);
        expect(applyCommands(before, alternative.commands).text, `turn ${n} changes the code`).not.toBe(before);
      }
    }
  );

  it.each(problems.map((p) => [p.id, p.turns] as const))(
    'given a rename turn of %s, when its transcript is parsed with the local fast path, then the local rename and the stub give the same text',
    (id, turns) => {
      // Given the turns whose answer is a rename (the app resolves those locally and ignores the planner's answer)
      const renames = replay(id, turns).filter(({ response }) => response.alternatives[0]?.commands[0]?.type === 'renameSymbol');
      expect(renames.length).toBeGreaterThan(0);

      for (const { n, response, before } of renames) {
        // When the transcript is parsed against the text before the turn
        const local = parseLocalEdit(response.transcript, before);

        // Then the app's local rename and the stub's alternative produce the same text
        expect(local, `turn ${n} is not a rename the app recognises`).not.toBeNull();
        expect(applyCommands(before, local!.commands).text, `turn ${n}`).toBe(applyCommands(before, response.alternatives[0]!.commands).text);
      }
    }
  );
});
