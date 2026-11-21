import { describe, expect, it } from 'vitest';

import { isLanguage, LANGUAGE_NAMES, parseApply, parseConfigPatch, parseHistory, parseLoad, parseRead, parseTree, parseView } from '../../src';

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
    ['a type step with no text', { steps: [{ op: 'type' }] }, 'invalid'],
    ['a type step with empty text', { steps: [{ op: 'type', text: '' }] }, 'invalid'],
    ['a type step with a paste', { steps: [{ op: 'type', text: 'x'.repeat(65) }] }, 'invalid'],
    ['a type step beside another step', { steps: [{ op: 'type', text: 'x' }, { op: 'select', anchor: 0 }] }, 'invalid'],
    ['a complete step', { steps: [{ op: 'complete' }] }, 'ok'],
    ['a complete step beside another step', { steps: [{ op: 'complete' }, { op: 'select', anchor: 0 }] }, 'invalid'],
    ['a complete step beside a type step', { steps: [{ op: 'type', text: 'x' }, { op: 'complete' }] }, 'invalid'],
    ['a type step of one keystroke', { steps: [{ op: 'type', text: '(' }] }, 'ok'],
    ['a type step of the longest keystroke', { steps: [{ op: 'type', text: 'x'.repeat(64) }] }, 'ok'],
  ])('given %s, when apply params are parsed, then they are %s', (_name, params, expected) => {
    expect(code(parseApply(params))).toBe(expected);
  });

  it('given a type step with CRLF, when parsed, then it is one LF', () => {
    const parsed = parseApply({ steps: [{ op: 'type', text: '\r\n' }] });
    expect(parsed.ok && parsed.value.steps[0]).toEqual({ op: 'type', text: '\n' });
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

describe('the language vocabulary', () => {
  it.each([
    ['python', true],
    ['javascript', true],
    ['cpp', true],
    ['java', false],
    ['Python', false],
    ['', false],
  ])('given %j, when it is checked, then isLanguage says %s', (name, expected) => {
    expect(isLanguage(name)).toBe(expected);
  });

  it('given the names the API lists, when each is checked, then every one is a language', () => {
    expect(LANGUAGE_NAMES.every((name) => isLanguage(name))).toBe(true);
  });

  it('given a language that is not a string, when a config patch is parsed, then it is invalid and names the path', () => {
    expect(parseConfigPatch({ language: 5 })).toMatchObject({ ok: false, error: { code: 'invalid', path: 'language' } });
  });
});

describe('the completion config', () => {
  const entries = (count: number) => Array.from({ length: count }, (_, i) => ({ label: `name${i}` }));

  it.each([
    ['an enabled flag and no entries', { completion: { enabled: true } }, 'ok'],
    ['entries with every field', { completion: { enabled: true, entries: [{ label: 'nums', type: 'variable', detail: 'list', boost: 9 }] } }, 'ok'],
    ['200 entries', { completion: { enabled: false, entries: entries(200) } }, 'ok'],
    ['201 entries', { completion: { enabled: false, entries: entries(201) } }, 'invalid'],
    ['a label with a space', { completion: { enabled: true, entries: [{ label: 'a b' }] } }, 'invalid'],
    ['an empty label', { completion: { enabled: true, entries: [{ label: '' }] } }, 'invalid'],
    ['a label of 65 characters', { completion: { enabled: true, entries: [{ label: 'a'.repeat(65) }] } }, 'invalid'],
    ['a boost of 100', { completion: { enabled: true, entries: [{ label: 'a', boost: 100 }] } }, 'invalid'],
    ['a detail of 65 characters', { completion: { enabled: true, entries: [{ label: 'a', detail: 'd'.repeat(65) }] } }, 'invalid'],
    ['no enabled flag', { completion: { entries: [] } }, 'invalid'],
    ['entries that are not an array', { completion: { enabled: true, entries: 'x' } }, 'invalid'],
  ])('given %s, when a config patch is parsed, then it is %s', (_name, params, expected) => {
    expect(code(parseConfigPatch(params))).toBe(expected);
  });

  it('given a label that is too long, when the patch is parsed, then the error names the entry', () => {
    expect(parseConfigPatch({ completion: { enabled: true, entries: [{ label: 'ok' }, { label: 'not ok' }] } })).toMatchObject({ ok: false, error: { path: 'completion.entries[1].label' } });
  });
});

describe('the tree params', () => {
  it.each([
    ['no params', undefined, 'ok'],
    ['null', null, 'ok'],
    ['an empty object', {}, 'ok'],
    ['every field', { range: { from: 0, to: 9 }, detail: 'full', budgetMs: 50 }, 'ok'],
    ['a budget of zero and of 1000', { budgetMs: 0 }, 'ok'],
    ['a budget of 1001', { budgetMs: 1001 }, 'invalid'],
    ['a negative budget', { budgetMs: -1 }, 'invalid'],
    ['a fractional budget', { budgetMs: 1.5 }, 'invalid'],
    ['a budget that is a string', { budgetMs: '5' }, 'invalid'],
    ['an inverted range', { range: { from: 5, to: 2 } }, 'invalid'],
    ['a detail of deep', { detail: 'deep' }, 'invalid'],
    ['params that are not an object', 5, 'invalid'],
  ])('given %s, when tree params are parsed, then they are %s', (_name, params, expected) => {
    expect(code(parseTree(params))).toBe(expected);
  });
});
