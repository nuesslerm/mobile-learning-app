import { ChangeSet, EditorSelection, Text } from '@codemirror/state';
import { describe, expect, it } from 'vitest';

import { applyChanges, invertChanges, mapPos, mapSelRange, type Change } from '../src';

const lcg = (seed: number) => {
  let state = seed >>> 0;
  return (limit: number) => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return Math.floor((state / 2 ** 32) * limit);
  };
};

const individual = (set: ChangeSet): Change[] => {
  const out: Change[] = [];
  set.iterChanges((from, to, _a, _b, text) => out.push({ from, to, insert: text.toString() }), true);
  return out;
};
const merged = (set: ChangeSet): Change[] => {
  const out: Change[] = [];
  set.iterChanges((from, to, _a, _b, text) => out.push({ from, to, insert: text.toString() }), false);
  return out;
};

function sample(next: (limit: number) => number) {
  const length = next(30);
  const doc = Array.from({ length }, () => 'abcdef\n'[next(7)]).join('');
  const cuts = Array.from({ length: next(4) * 2 }, () => next(length + 1)).sort((a, b) => a - b);
  const specs: { from: number; to: number; insert: string }[] = [];
  for (let i = 0; i + 1 < cuts.length; i += 2) specs.push({ from: cuts[i] as number, to: cuts[i + 1] as number, insert: 'xyz\n'.slice(0, next(4)) });
  return { doc, specs };
}

describe('the pure mapping against CodeMirror ChangeSet and SelectionRange', () => {
  it('given random documents and change sets, when each function runs on the individual change list, then it equals CodeMirror at every position and association', () => {
    const next = lcg(7);
    let checks = 0;
    for (let round = 0; round < 500; round += 1) {
      // Given a random document and a random simultaneous change set
      const { doc, specs } = sample(next);
      const set = ChangeSet.of(specs, doc.length);
      const changes = individual(set);
      const text = Text.of(doc.split('\n'));

      // Then applying, inverting and mapping all agree with CodeMirror
      expect(applyChanges(doc, changes)).toBe(set.apply(text).toString());
      const inverse = set.invert(text);
      expect(invertChanges(doc, changes)).toEqual(individual(inverse));
      for (let pos = 0; pos <= doc.length; pos += 1) {
        for (const assoc of [-1, 1] as const) {
          expect(mapPos(pos, changes, assoc)).toBe(set.mapPos(pos, assoc));
          checks += 1;
        }
      }
      for (let anchor = 0; anchor <= doc.length; anchor += 1) {
        for (let head = 0; head <= doc.length; head += 1) {
          const real = EditorSelection.range(anchor, head).map(set);
          const mine = mapSelRange({ anchor, head }, changes);
          // the one deliberate difference: a range turned inside out collapses to a caret after the replacement
          if (real.from > real.to) {
            expect(mine.anchor).toBe(mine.head);
            expect(mine.head).toBe(Math.max(real.from, real.to) === real.from ? real.from : real.to);
          } else {
            expect(mine).toEqual({ anchor: real.anchor, head: real.head });
          }
          checks += 1;
        }
      }
    }
    expect(checks).toBeGreaterThan(30_000);
  }, 60_000);

  it('given a merged change list, when positions map through it, then some boundaries disagree with CodeMirror, which is why every list stays individual', () => {
    const next = lcg(3);
    let disagreements = 0;
    for (let round = 0; round < 1500; round += 1) {
      const { doc, specs } = sample(next);
      const set = ChangeSet.of(specs, doc.length);
      const wrong = merged(set);
      for (let pos = 0; pos <= doc.length; pos += 1) for (const assoc of [-1, 1] as const) if (mapPos(pos, wrong, assoc) !== set.mapPos(pos, assoc)) disagreements += 1;
    }
    expect(disagreements).toBeGreaterThan(0);
  });
});
