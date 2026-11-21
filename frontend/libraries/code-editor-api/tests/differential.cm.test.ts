import { describe, expect, it } from 'vitest';

import { createEditorClient, type EditorClient } from '../src';
import { createFakeEditor, type FakeEditor } from '../src/testing';
import { createHeadlessCm } from './support/headlessCmHost';

const lcg = (seed: number) => {
  let state = seed >>> 0;
  return (limit: number) => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return Math.floor((state / 2 ** 32) * limit);
  };
};

const WORDS = ['', 'a', 'xy', 'foo ', '\n', '  ', 'q\nr'];
const SEED_TEXT = 'hello world\nsecond line\n';

type Roll = { kind: number; a: number; b: number; c: number; text: string; flag: boolean; flag2: boolean; back: number };
type Editors = readonly [FakeEditor, ReturnType<typeof createHeadlessCm>];

function operation(roll: Roll, head: number): (client: EditorClient, editor: FakeEditor) => Promise<unknown> | void {
  const { a, b, c, text, flag, flag2, back } = roll;
  const low = Math.min(a, b);
  const high = Math.max(a, b);
  switch (roll.kind) {
    case 0: return (cl) => cl.replaceRange({ from: low, to: high }, text);
    case 1: return (cl) => cl.insertAt(a, text);
    case 2: return (cl) => cl.transact([{ op: 'replace', range: { from: low, to: high }, text }, { op: 'replace', range: { from: c, to: c }, text: 'Z' }]);
    case 3: return (cl) => cl.moveRange({ from: low, to: high }, c);
    case 4: return (cl) => cl.select(a, flag ? b : undefined);
    case 5: return (cl) => cl.transact([{ op: 'replace', range: { from: low, to: high }, text }, { op: 'select', anchor: c, head: a }]);
    case 6: return (cl) => cl.transact([{ op: 'replace', range: { from: low, to: high }, text: text || 'k' }, { op: 'select', anchor: { step: 0, edge: 'end' }, head: { step: 0 } }]);
    case 7: return (cl) => cl.transact([{ op: 'replace', range: { from: a, to: a }, text: 'W' }, { op: 'select', anchor: { at: c, assoc: flag ? 1 : -1 } }]);
    case 8: return (cl) => cl.undo();
    case 9: return (cl) => cl.redo();
    case 10: return (_cl, ed) => ed.userTypes(text || 't');
    case 11: return (_cl, ed) => ed.userSelects(a, flag ? a : b);
    case 12: return (_cl, ed) => ed.advance(flag ? 100 : 700);
    case 13: return (cl) => cl.load(flag2 ? text : SEED_TEXT, flag ? { anchor: 0, head: Math.min(1, (flag2 ? text : SEED_TEXT).length) } : undefined);
    default: return (cl) => cl.transact([{ op: 'replace', range: { from: low, to: high }, text }], { at: Math.max(0, head - back), stale: flag ? 'map' : 'reject' });
  }
}

/** One seeded script: every random draw is made once, then the same operation runs on both editors, and everything observable must agree. */
type Stats = { applied: number; refused: number; undone: number; redone: number };

async function runScript(seed: number, operations: number, stats: Stats): Promise<string | null> {
  const next = lcg(seed);
  const editors: Editors = [createFakeEditor({ text: SEED_TEXT }), createHeadlessCm({ text: SEED_TEXT })];
  const clients = [createEditorClient(editors[0].engine), createEditorClient(editors[1].engine)] as const;
  for (let i = 0; i < operations; i += 1) {
    const length = editors[0].peek().text.length;
    const at = () => next(length + 1);
    const roll: Roll = { kind: next(15), a: at(), b: at(), c: at(), text: WORDS[next(WORDS.length)] as string, flag: next(2) === 1, flag2: next(2) === 1, back: next(3) };
    roll.b = Math.min(length, roll.a + next(5));
    const beforeView = JSON.stringify([editors[0].peek().text, editors[0].peek().selection, editors[1].peek().text, editors[1].peek().selection]);
    const run = operation(roll, editors[0].peek().revision);
    const results = [await run(clients[0], editors[0]), await run(clients[1], editors[1])];
    const answer = results[1] as { ok?: boolean; value?: { applied?: boolean } } | undefined;
    if (roll.kind <= 7 || roll.kind === 14) stats[answer?.ok ? 'applied' : 'refused'] += 1;
    if (roll.kind === 8 && answer?.value?.applied) stats.undone += 1;
    if (roll.kind === 9 && answer?.value?.applied) stats.redone += 1;
    const [fake, real] = [editors[0].peek(), editors[1].peek()];
    const view = (peek: typeof fake) => JSON.stringify([peek.text, peek.selection, peek.revision]);
    // CodeMirror reports a composed undo as split hunks: the contract promises text and revision accounting for a history change, not its exact ranges
    const shown = (result: unknown) => JSON.stringify(result, (key, value: unknown) => (key === 'changes' && roll.kind >= 8 && roll.kind <= 9 ? undefined : value));
    if (view(fake) !== view(real) || shown(results[0]) !== shown(results[1])) {
      return `seed ${seed} step ${i} kind ${roll.kind} ${JSON.stringify(roll)} diverged from ${beforeView}\nfake: ${view(fake)} ${JSON.stringify(results[0])}\ncm:   ${view(real)} ${JSON.stringify(results[1])}`;
    }
  }
  return null;
}

describe('the fake editor against real CodeMirror state and history', () => {
  it('given seeded random scripts, when both run, then text, selection, revision and every reply agree after each operation', async () => {
    const stats: Stats = { applied: 0, refused: 0, undone: 0, redone: 0 };
    for (const seed of [1, 2, 3, 4]) expect(await runScript(seed, 4000, stats)).toBeNull();
    // the scripts must actually reach the interesting paths, or agreement proves nothing
    expect(stats.applied).toBeGreaterThan(2000);
    expect(stats.refused).toBeGreaterThan(200);
    expect(stats.undone).toBeGreaterThan(500);
    expect(stats.redone).toBeGreaterThan(50);
  }, 120_000);
});
