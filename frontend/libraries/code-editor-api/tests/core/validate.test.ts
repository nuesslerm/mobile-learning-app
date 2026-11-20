import { describe, expect, it } from 'vitest';

import { parseApply, parseConfigPatch, parseHistory, parseLoad, parseRead, parseView } from '../../src';

const code = (parsed: { ok: boolean; error?: { code: string } }) => (parsed.ok ? 'ok' : parsed.error?.code);

describe('the wire validators', () => {
  it.each([
    ['no steps', {}, 'invalid'],
    ['steps that are not an array', { steps: 'x' }, 'invalid'],
    ['a step with no op', { steps: [{}] }, 'invalid'],
    ['an unknown op', { steps: [{ op: 'frobnicate' }] }, 'unsupported'],
    ['a fractional position', { steps: [{ op: 'replace', range: { from: 1.5, to: 2 }, text: '' }] }, 'invalid'],
    ['a negative position', { steps: [{ op: 'replace', range: { from: -1, to: 2 }, text: '' }] }, 'invalid'],
    ['an inverted range', { steps: [{ op: 'replace', range: { from: 3, to: 2 }, text: '' }] }, 'invalid'],
    ['text that is not a string', { steps: [{ op: 'replace', range: 'selection', text: 5 }] }, 'invalid'],
    ['a point at a missing step', { steps: [{ op: 'select', anchor: { step: 3 } }] }, 'invalid'],
    ['a point at a select step', { steps: [{ op: 'select', anchor: 0 }, { op: 'select', anchor: { step: 0 } }] }, 'invalid'],
    ['an at that is not an integer', { steps: [], at: 'x' }, 'invalid'],
    ['a stale policy that is neither', { steps: [], stale: 'maybe' }, 'invalid'],
    ['an illegal label', { steps: [], label: 'Bad Label' }, 'invalid'],
  ])('given %s, when apply params are parsed, then they are %s', (_name, params, expected) => {
    expect(code(parseApply(params))).toBe(expected);
  });

  it('given step text with CRLF, when parsed, then text and expect are normalised', () => {
    const parsed = parseApply({ steps: [{ op: 'replace', range: { from: 0, to: 0 }, text: 'a\r\nb\rc', expect: 'x\r\ny' }] });
    expect(parsed.ok && parsed.value.steps[0]).toEqual({ op: 'replace', range: { from: 0, to: 0 }, text: 'a\nb\nc', expect: 'x\ny' });
  });

  it('given a valid batch with options, when parsed, then nothing is dropped and no undefined field appears', () => {
    const params = { steps: [{ op: 'select', anchor: { at: 2, assoc: -1 }, head: { step: 1, edge: 'end', offset: 1 } }, { op: 'replace', range: 'selection', text: 'x' }], at: 4, stale: 'map', instance: 'i', label: 'a.b' };
    const parsed = parseApply(params);
    expect(parsed.ok && JSON.parse(JSON.stringify(parsed.value))).toEqual(params);
  });

  it.each([
    ['read with a bad range', () => parseRead({ range: { from: 'a', to: 1 } })],
    ['history with a bad direction', () => parseHistory({ direction: 'sideways' })],
    ['load without text', () => parseLoad({})],
    ['view with a non-boolean focus', () => parseView({ focus: 'yes' })],
    ['configure with a zero tab size', () => parseConfigPatch({ tabSize: 0 })],
    ['configure with an infinite font size', () => parseConfigPatch({ fontSize: Infinity })],
    ['configure with a long colour', () => parseConfigPatch({ palette: { cursor: 'x'.repeat(200) } })],
  ])('given %s, when parsed, then it is invalid', (_name, parse) => {
    expect(code(parse())).toBe('invalid');
  });
});
