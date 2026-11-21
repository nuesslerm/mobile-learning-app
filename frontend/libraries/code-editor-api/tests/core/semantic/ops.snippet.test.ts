import { describe, expect, it } from 'vitest';

import { type Range, type Step } from '../../../src';
import { lezerSnapshot } from '../../support/lezerPlain';
import { between, outcomeOf, PARSERS, previewOf } from '../../support/semanticPlan';

type Destination = Extract<Step, { op: 'insertSnippet' }>['at'];
const snippet = (at: Destination, text: string): Step => ({ op: 'insertSnippet', at, text });

const PY = 'def f(x):\n    if x:\n        a = 1\n        b = 2\n    return b\n';
const JS = 'function f(x) {\n    if (x) {\n        a = 1;\n        b = 2;\n    }\n    return b;\n}\n';

describe.each(PARSERS)('insertSnippet planned over %s', (_name, parser) => {
  it('given a snippet that carries its own first-line indent, when it is inserted before a statement, then it is dedented and every line takes the destination indent on top of its relative one', async () => {
    const python = await previewOf(parser, 'python', PY, [snippet({ before: between(PY, 'b = 2') }, '      if y:\n          c = 3\n      d = 4')]);
    const braces = await previewOf(parser, 'javascript', JS, [snippet({ before: between(JS, 'b = 2;') }, '  if (y) {\n    c = 3;\n  }\n  d = 4;')]);

    expect(outcomeOf(python)).toBe('def f(x):\n    if x:\n        a = 1\n        if y:\n            c = 3\n        d = 4\n        b = 2\n    return b\n');
    expect(outcomeOf(braces)).toBe('function f(x) {\n    if (x) {\n        a = 1;\n        if (y) {\n          c = 3;\n        }\n        d = 4;\n        b = 2;\n    }\n    return b;\n}\n');
  });

  it('given a snippet with a blank line, a trailing newline and CRLF line ends, when it is inserted, then the blank line stays empty, there is one newline at the end and the text is LF', async () => {
    const reply = await previewOf(parser, 'python', PY, [snippet({ after: between(PY, 'a = 1') }, 'c = 3\r\n\r\nd = 4\r\n')]);

    expect(outcomeOf(reply)).toBe('def f(x):\n    if x:\n        a = 1\n        c = 3\n\n        d = 4\n        b = 2\n    return b\n');
  });

  it('given a document with no final newline, when a snippet is inserted after its last statement, then the snippet follows a newline and the text still has no final newline', async () => {
    const text = 'a = 1\nb = 2';

    const reply = await previewOf(parser, 'python', text, [snippet({ after: between(text, 'b = 2') }, 'c = 3')]);

    expect(outcomeOf(reply)).toBe('a = 1\nb = 2\nc = 3');
  });

  it.each([
    ['python', PY, between(PY, 'if x:', 'b = 2'), 'start', 'def f(x):\n    if x:\n        c = 3\n        a = 1\n        b = 2\n    return b\n'],
    ['python', PY, between(PY, 'if x:', 'b = 2'), 'end', 'def f(x):\n    if x:\n        a = 1\n        b = 2\n        c = 3\n    return b\n'],
  ] as const)('given a %s block, when a snippet goes to its %s, then it lands at that end at the indent of the statements there', async (language, text, into, at, expected) => {
    const reply = await previewOf(parser, language, text, [snippet({ into, at }, 'c = 3')]);

    expect(outcomeOf(reply)).toBe(expected);
  });

  it('given an empty braces block, when a snippet goes into it, then the braces open at the owner indent plus one unit', async () => {
    const text = 'function f(x) {\n    if (x) {}\n    return 1;\n}\n';

    const reply = await previewOf(parser, 'javascript', text, [snippet({ into: between(text, 'if (x) {}'), at: 'end' }, 'g();\nh();')]);

    expect(outcomeOf(reply)).toBe('function f(x) {\n    if (x) {\n        g();\n        h();\n    }\n    return 1;\n}\n');
  });
});

describe('insertSnippet planned over the installed grammars', () => {
  const lezer = lezerSnapshot;

  it.each([
    ['python', PY, 'x = ('],
    ['javascript', JS, 'x = (;'],
  ] as const)('given a %s snippet that does not parse, when it is inserted, then it is would-not-parse syntax with an error inside the would-be text and nothing is applied', async (language, text, code) => {
    const at = { after: between(text, language === 'python' ? 'b = 2' : 'b = 2;') };

    const reply = await previewOf(lezer, language, text, [snippet(at, code)]);

    expect(outcomeOf(reply)).toMatchObject({ code: 'would-not-parse', reason: 'syntax' });
    expect(!reply.ok && reply.error.code === 'would-not-parse' && reply.error.errors.every((range: Range) => range.to <= text.length + code.length + 16)).toBe(true);
  });

  it.each([
    ['python', PY, 'elif y:\n    c = 3'],
    ['javascript', JS, 'else if (y) {\n  c = 3;\n}'],
  ] as const)('given a %s snippet that is a clause of the if before it, when it is inserted after the if, then it is would-not-parse structure', async (language, text, code) => {
    const reply = await previewOf(lezer, language, text, [snippet({ after: between(text, language === 'python' ? 'if x:' : 'if (x) {', language === 'python' ? 'b = 2' : '    }') }, code)]);

    expect(outcomeOf(reply)).toMatchObject({ code: 'would-not-parse', reason: 'structure' });
  });

  it('given a snippet over a destination that is a braceless body, when it is inserted there, then it is not-a-node not-in-block naming the destination', async () => {
    const text = 'function f(x) {\n  if (x)\n    a = 1;\n}\n';

    const reply = await previewOf(lezer, 'javascript', text, [snippet({ before: between(text, 'a = 1;') }, 'b = 2;')]);

    expect(outcomeOf(reply)).toEqual({ code: 'not-a-node', reason: 'not-in-block', range: between(text, 'a = 1;') });
  });
});
