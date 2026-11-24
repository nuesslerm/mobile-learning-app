import { describe, expect, it } from 'vitest';

import { createEditorClient, unwrap, type Language, type Step } from '../../../src';
import { createFakeEditor } from '../../../src/testing';
import { lezerSnapshot } from '../../support/lezerPlain';
import { between, outcomeOf, PARSERS, previewOf } from '../../support/semanticPlan';

const wrap = (node: ReturnType<typeof between> | 'selection', header: string): Step => ({ op: 'wrapNode', node, header });
const unwrapStep = (node: ReturnType<typeof between>): Step => ({ op: 'unwrapNode', node });

const PY = 'def f(x):\n    if x:\n        a = 1\n        b = 2\n    return b\n';
const JS = 'function f(x) {\n  if (x) {\n    a = 1;\n    b = 2;\n  }\n  return b;\n}\n';
const CPP = 'int f(int x) {\n  if (x) {\n    a = 1;\n    b = 2;\n  }\n  return b;\n}\n';

describe('wrapNode on the installed grammars', () => {
  const lezer = lezerSnapshot;

  it.each([
    ['for i in', 'syntax'],
    ['for i in range(2):', 'syntax'],
    ['if x: foo()', 'syntax'],
  ])('given the python header %j, when two statements are wrapped, then it is would-not-parse %s with an error in the would-be text', async (header, reason) => {
    const reply = await previewOf(lezer, 'python', PY, [wrap(between(PY, 'a = 1', 'b = 2'), header)]);

    const outcome = outcomeOf(reply);
    expect(outcome).toMatchObject({ code: 'would-not-parse', reason });
    expect(!reply.ok && reply.error.code === 'would-not-parse' && reply.error.errors.length).toBeGreaterThan(0);
  });

  it.each(['else', 'elif y'])('given the python header %j, when two statements are wrapped, then it is refused and nothing is applied', async (header) => {
    const reply = await previewOf(lezer, 'python', PY, [wrap(between(PY, 'a = 1', 'b = 2'), header)]);

    expect(outcomeOf(reply)).toMatchObject({ code: 'would-not-parse' });
  });

  it.each(['else', 'if (x) foo();', 'for ('])('given the javascript header %j, when two statements are wrapped, then it is would-not-parse', async (header) => {
    const reply = await previewOf(lezer, 'javascript', JS, [wrap(between(JS, 'a = 1;', 'b = 2;'), header)]);

    expect(outcomeOf(reply)).toMatchObject({ code: 'would-not-parse' });
  });

  it.each([
    ['python', PY, between(PY, 'a = 1', 'b = 2')],
    ['javascript', JS, between(JS, 'a = 1;', 'b = 2;')],
    ['cpp', CPP, between(CPP, 'a = 1;', 'b = 2;')],
  ] as const)('given a try header in %s, when two statements are wrapped, then it is invalid naming the header, because a try needs a clause one line cannot give', async (language, text, node) => {
    const reply = await previewOf(lezer, language, text, [wrap(node, 'try')]);

    expect(outcomeOf(reply)).toEqual({ code: 'invalid', path: 'steps[0].header' });
  });

  it('given a python run with a triple-quoted string, when it is wrapped, then the inner lines of the string keep their text', async () => {
    const text = 'def f():\n    s = """\n  inner\n    low"""\n    return s\n';

    const reply = await previewOf(lezer, 'python', text, [wrap(between(text, 's = """', 'low"""'), 'if True')]);

    expect(outcomeOf(reply)).toBe('def f():\n    if True:\n        s = """\n  inner\n    low"""\n    return s\n');
  });

  it('given an empty selection in a statement, when it is wrapped, then the statement at the caret goes under the header', async () => {
    const fake = createFakeEditor({ text: PY, tree: lezer });
    const client = createEditorClient(fake.engine);
    unwrap(await client.select(PY.indexOf('b = 2') + 2));

    unwrap(await client.wrapNode('selection', 'while x'));

    expect(fake.text()).toBe('def f(x):\n    if x:\n        a = 1\n        while x:\n            b = 2\n    return b\n');
    expect(fake.peek().selection.ranges[0]?.head).toBe(fake.text().indexOf('b = 2') + 2);
  });

  it('given a braceless body, when it is wrapped, then it is not-a-node not-in-block naming it', async () => {
    const text = 'function f(x) {\n  if (x)\n    a = 1;\n}\n';

    const reply = await previewOf(lezer, 'javascript', text, [wrap(between(text, 'a = 1;'), 'for (;;)')]);

    expect(outcomeOf(reply)).toEqual({ code: 'not-a-node', reason: 'not-in-block', range: between(text, 'a = 1;') });
  });
});

describe('unwrapNode on the installed grammars', () => {
  const lezer = lezerSnapshot;

  it.each([
    ['python', 'def f(x):\n    if x: a = 1\n    return 1\n', 'if x: a = 1', 'shares-line'],
    ['javascript', 'function f(x) {\n  if (x) { a(); }\n  return 1;\n}\n', 'if (x) { a(); }', 'shares-line'],
    ['javascript', 'function f(x) {\n  if (a) {\n    x();\n  } else if (b) {\n    y();\n  }\n}\n', 'if (a) {\n    x();\n  } else if (b) {\n    y();\n  }', 'not-unwrappable'],
    ['javascript', 'function f(x) {\n  do {\n    g();\n  } while (x);\n}\n', 'do {\n    g();\n  } while (x);', 'not-unwrappable'],
    ['javascript', 'function f(x) {\n  try {\n    g();\n  } catch (e) {\n    h();\n  }\n}\n', 'try {\n    g();\n  } catch (e) {\n    h();\n  }', 'not-unwrappable'],
    ['javascript', 'function f(x) {\n  for (;;) {}\n}\n', 'for (;;) {}', 'not-unwrappable'],
    ['python', 'def f(xs):\n    for x in xs:\n        a = 1\n    else:\n        b = 2\n', 'for x in xs:\n        a = 1\n    else:\n        b = 2', 'not-unwrappable'],
    ['python', '@cache\ndef f(x):\n    return x\n', '@cache\ndef f(x):\n    return x', 'not-unwrappable'],
    ['cpp', 'struct A {\n  int a;\n};\n', 'struct A {\n  int a;\n};', 'no-statement'],
  ] as const)('given %s code with a compound that cannot be taken apart (%#), when it is unwrapped, then it is not-a-node %s naming it and nothing is applied', async (language, text, node, reason) => {
    const reply = await previewOf(lezer, language, text, [unwrapStep(between(text, node))]);

    expect(outcomeOf(reply)).toEqual({ code: 'not-a-node', reason, range: between(text, node) });
  });

  it('given a comment line above a loop, when the loop is unwrapped, then the comment goes with the header', async () => {
    const text = 'def f(xs):\n    # each one\n    for x in xs:\n        print(x)\n    return 1\n';

    const reply = await previewOf(lezer, 'python', text, [unwrapStep(between(text, 'for x in xs:', 'print(x)'))]);

    expect(outcomeOf(reply)).toBe('def f(xs):\n    print(x)\n    return 1\n');
  });

  it('given a python def at the top of the file, when it is unwrapped, then its body is at the left margin', async () => {
    const text = 'def f():\n    a = 1\n    return a\n';

    const reply = await previewOf(lezer, 'python', text, [unwrapStep(between(text, 'def f():', 'return a'))]);

    expect(outcomeOf(reply)).toBe('a = 1\nreturn a\n');
  });

  it('given a python loop whose body holds a triple-quoted string, when it is unwrapped, then the inner lines of the string keep their text', async () => {
    const text = 'def f(xs):\n    for x in xs:\n        s = """\n  inner\n    low"""\n    return s\n';

    const reply = await previewOf(lezer, 'python', text, [unwrapStep(between(text, 'for x in xs:', 'low"""'))]);

    expect(outcomeOf(reply)).toBe('def f(xs):\n    s = """\n  inner\n    low"""\n    return s\n');
  });

  it('given a loop that ends a document with no final newline, when it is unwrapped, then the body is the end of the document and has no final newline', async () => {
    const text = 'function f() {\n}\nfor (;;) {\n    a();\n}';

    const reply = await previewOf(lezer, 'javascript', text, [unwrapStep({ from: text.indexOf('for (;;)'), to: text.length })]);

    expect(outcomeOf(reply)).toBe('function f() {\n}\na();');
  });

  it('given the caret in the body of a loop, when it is unwrapped, then the caret is on the same character in the new place', async () => {
    const text = 'def f(xs):\n    for x in xs:\n        print(x)\n    return 1\n';
    const fake = createFakeEditor({ text, tree: lezerSnapshot });
    const client = createEditorClient(fake.engine);
    unwrap(await client.select(text.indexOf('print(x)') + 3));

    unwrap(await client.unwrapNode(between(text, 'for x in xs:', 'print(x)')));

    expect(fake.text()).toBe('def f(xs):\n    print(x)\n    return 1\n');
    expect(fake.peek().selection.ranges[0]?.head).toBe(fake.text().indexOf('print(x)') + 3);
  });
});

describe.each(PARSERS)('wrapNode and unwrapNode planned over %s', (_name, parser) => {
  it.each([
    ['python', PY, between(PY, 'b = 2'), 'while x'],
    ['javascript', JS, between(JS, 'b = 2;'), 'while (x)'],
  ] as const)('given a statement in %s, when it is wrapped and the new compound is unwrapped, then the preview shows the original text', async (language: Language, text, node, header) => {
    const wrapped = await previewOf(parser, language, text, [wrap(node, header)]);
    const text2 = outcomeOf(wrapped);
    if (typeof text2 !== 'string') throw new Error('the wrap was refused');
    const compound = between(text2, header, language === 'python' ? 'b = 2' : '    }');

    const back = await previewOf(parser, language, text2, [unwrapStep(compound)]);

    expect(outcomeOf(back)).toBe(text);
  });
});
