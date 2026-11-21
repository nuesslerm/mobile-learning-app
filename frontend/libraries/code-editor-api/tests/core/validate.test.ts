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
    ['a moveNode with a before destination', { steps: [{ op: 'moveNode', node: { from: 0, to: 5 }, to: { before: { from: 9, to: 12 } } }] }, 'ok'],
    ['a moveNode with no to', { steps: [{ op: 'moveNode', node: { from: 0, to: 5 } }] }, 'invalid'],
    ['a destination with two keys', { steps: [{ op: 'moveNode', node: { from: 0, to: 5 }, to: { before: { from: 9, to: 12 }, after: { from: 9, to: 12 } } }] }, 'invalid'],
    ['an into destination with an at of middle', { steps: [{ op: 'moveNode', node: { from: 0, to: 5 }, to: { into: { from: 9, to: 12 }, at: 'middle' } }] }, 'invalid'],
    ['an into destination with a fractional block', { steps: [{ op: 'insertSnippet', at: { into: { from: 9, to: 12 }, at: 'end', block: 1.5 }, text: 'x' }] }, 'invalid'],
    ['a wrapNode with a header holding a newline', { steps: [{ op: 'wrapNode', node: { from: 0, to: 5 }, header: 'for i in a\nfor j in b' }] }, 'invalid'],
    ['a wrapNode with an empty header', { steps: [{ op: 'wrapNode', node: { from: 0, to: 5 }, header: '' }] }, 'invalid'],
    ['a wrapNode with a header of 201 characters', { steps: [{ op: 'wrapNode', node: { from: 0, to: 5 }, header: 'x'.repeat(201) }] }, 'invalid'],
    ['an insertSnippet with blank text', { steps: [{ op: 'insertSnippet', at: { after: { from: 0, to: 5 } }, text: ' \n ' }] }, 'invalid'],
    ['an insertSnippet of 4001 characters', { steps: [{ op: 'insertSnippet', at: { after: { from: 0, to: 5 } }, text: 'x'.repeat(4001) }] }, 'invalid'],
    ['a reindentBlock by 9', { steps: [{ op: 'reindentBlock', node: { from: 0, to: 5 }, by: 9 }] }, 'invalid'],
    ['a reindentBlock by 1.5', { steps: [{ op: 'reindentBlock', node: { from: 0, to: 5 }, by: 1.5 }] }, 'invalid'],
    ['a reindentBlock by -8', { steps: [{ op: 'reindentBlock', node: { from: 0, to: 5 }, by: -8 }] }, 'ok'],
    ['a deleteNode with node selection', { steps: [{ op: 'deleteNode', node: 'selection' }] }, 'ok'],
    ['a deleteNode with no node', { steps: [{ op: 'deleteNode' }] }, 'invalid'],
    ['a node step beside a replace step', { steps: [{ op: 'deleteNode', node: 'selection' }, { op: 'replace', range: { from: 0, to: 0 }, text: 'x' }] }, 'invalid'],
    ['a node step beside a move step', { steps: [{ op: 'duplicateNode', node: 'selection' }, { op: 'move', range: { from: 0, to: 1 }, to: 3 }] }, 'invalid'],
    ['a node step beside one select step', { steps: [{ op: 'deleteNode', node: 'selection' }, { op: 'select', anchor: { step: 0 } }] }, 'ok'],
    ['a preview that is not a boolean', { steps: [], preview: 'yes' }, 'invalid'],
    ['a preview of a replace step', { steps: [{ op: 'replace', range: { from: 0, to: 0 }, text: 'x' }], preview: true }, 'ok'],
    ['a type step with a preview', { steps: [{ op: 'type', text: 'x' }], preview: true }, 'invalid'],
    ['a complete step with a preview', { steps: [{ op: 'complete' }], preview: true }, 'invalid'],
    ['a budget of 1001', { steps: [], budgetMs: 1001 }, 'invalid'],
    ['a budget of -1', { steps: [], budgetMs: -1 }, 'invalid'],
    ['a budget of 0 and of 1000', { steps: [], budgetMs: 0 }, 'ok'],
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

  it('given every node step with every field, when parsed, then nothing is dropped and no undefined field appears', () => {
    const node = { from: 2, to: 9 };
    const params = {
      steps: [
        { op: 'moveNode', node, to: { into: { from: 12, to: 20 }, at: 'end', block: 1 }, expect: 'x' },
        { op: 'wrapNode', node, header: 'for i in range(n)', expect: 'x' },
        { op: 'unwrapNode', node: 'selection' },
        { op: 'deleteNode', node },
        { op: 'duplicateNode', node, expect: 'x' },
        { op: 'insertSnippet', at: { after: node }, text: 'a = 1\nb = 2' },
        { op: 'reindentBlock', node, by: -1 },
        { op: 'reindentBlock', node },
      ],
      at: 3,
      preview: true,
      budgetMs: 250,
    };
    const parsed = parseApply(params);
    expect(parsed.ok && JSON.parse(JSON.stringify(parsed.value))).toEqual(params);
  });

  it('given a snippet with CRLF, when parsed, then it is LF', () => {
    const parsed = parseApply({ steps: [{ op: 'insertSnippet', at: { before: { from: 0, to: 0 } }, text: 'a\r\nb' }] });
    expect(parsed.ok && parsed.value.steps[0]).toMatchObject({ text: 'a\nb' });
  });

  it('given a node step that fails, when parsed, then the error names its path', () => {
    expect(parseApply({ steps: [{ op: 'deleteNode', node: 'selection' }, { op: 'moveNode', node: 'selection', to: { into: { from: 0, to: 0 }, at: 'middle' } }] })).toMatchObject({ ok: false, error: { path: 'steps[1].to.at' } });
    expect(parseApply({ steps: [{ op: 'reindentBlock', node: 'selection', by: 9 }] })).toMatchObject({ ok: false, error: { path: 'steps[0].by' } });
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
