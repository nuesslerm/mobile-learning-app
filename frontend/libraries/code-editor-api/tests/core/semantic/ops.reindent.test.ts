import { describe, expect, it } from 'vitest';

import { type Step } from '../../../src';
import { lezerSnapshot } from '../../support/lezerPlain';
import { between, outcomeOf, PARSERS, previewOf } from '../../support/semanticPlan';

const reindent = (node: ReturnType<typeof between>, by?: number): Step => (by === undefined ? { op: 'reindentBlock', node } : { op: 'reindentBlock', node, by });

describe.each(PARSERS)('reindentBlock planned over %s', (_name, parser) => {
  it('given an if with an else pasted with a shallower body, when it is re-indented with no count, then both bodies are at their depth and the else line is at the depth of the if', async () => {
    const text = 'def f(x):\n    if x:\n      a = 1\n    else:\n      b = 2\n    return b\n';

    const reply = await previewOf(parser, 'python', text, [reindent(between(text, 'if x:', 'b = 2'))]);

    expect(outcomeOf(reply)).toBe('def f(x):\n    if x:\n        a = 1\n    else:\n        b = 2\n    return b\n');
  });

  it('given a run that is the whole block of a function, when it is re-indented with no count, then it takes one unit inside the function', async () => {
    const python = 'def f(x):\n  a = 1\n  b = 2\n';
    const braces = 'function f(x) {\n  a = 1;\n  b = 2;\n}\n';

    const first = await previewOf(parser, 'python', python, [reindent(between(python, 'a = 1', 'b = 2'))]);
    const second = await previewOf(parser, 'javascript', braces, [reindent(between(braces, 'a = 1;', 'b = 2;'))]);

    expect(outcomeOf(first)).toBe('def f(x):\n    a = 1\n    b = 2\n');
    expect(outcomeOf(second)).toBe('function f(x) {\n    a = 1;\n    b = 2;\n}\n');
  });

  it('given statements at the left margin that are indented, when they are re-indented with no count, then they sit at the margin', async () => {
    const text = '    a = 1;\n    for (;;) {\n  g();\n        }\n';

    const reply = await previewOf(parser, 'javascript', text, [reindent(between(text, 'a = 1;', '        }'))]);

    expect(outcomeOf(reply)).toBe('a = 1;\nfor (;;) {\n    g();\n}\n');
  });

  it('given nested blocks pasted with the wrong depths, when they are re-indented with no count, then every line has one unit for each block it is inside', async () => {
    const text = 'function f(x) {\n    if (x) {\n for (;;) {\n    g();\n      }\n   }\n    return 1;\n}\n';

    const reply = await previewOf(parser, 'javascript', text, [reindent({ from: text.indexOf('if (x) {'), to: text.indexOf('\n    return 1;') })]);

    expect(outcomeOf(reply)).toBe('function f(x) {\n    if (x) {\n        for (;;) {\n            g();\n        }\n    }\n    return 1;\n}\n');
  });

  it('given a count of 0 or a block already at its depth, when it is re-indented, then it is ok with no change', async () => {
    const text = 'def f(x):\n    a = 1\n    b = 2\n';

    const counted = await previewOf(parser, 'python', text, [reindent(between(text, 'b = 2'), 0)]);
    const normal = await previewOf(parser, 'python', text, [reindent(between(text, 'a = 1', 'b = 2'))]);

    expect(outcomeOf(normal)).toBe(text);
    const reply = counted;

    expect(outcomeOf(reply)).toBe(text);
  });

  it('given braces, when the last statement of an if body is outdented, then it stays in the block, and when it is indented it stays too', async () => {
    const text = 'function f(x) {\n    if (x) {\n        a = 1;\n        b = 2;\n    }\n}\n';

    const out = await previewOf(parser, 'javascript', text, [reindent(between(text, 'b = 2;'), -1)]);
    const further = await previewOf(parser, 'javascript', text, [reindent(between(text, 'b = 2;'), 1)]);

    expect(outcomeOf(out)).toBe('function f(x) {\n    if (x) {\n        a = 1;\n    b = 2;\n    }\n}\n');
    expect(outcomeOf(further)).toBe('function f(x) {\n    if (x) {\n        a = 1;\n            b = 2;\n    }\n}\n');
  });
});

describe('reindentBlock planned over the installed grammars', () => {
  const lezer = lezerSnapshot;

  it('given the last statement of a python if body, when it is outdented by one unit, then it leaves the body and the text parses', async () => {
    const text = 'def f(x):\n    if x:\n        a = 1\n        b = 2\n    return b\n';

    const reply = await previewOf(lezer, 'python', text, [reindent(between(text, 'b = 2'), -1)]);

    expect(outcomeOf(reply)).toBe('def f(x):\n    if x:\n        a = 1\n    b = 2\n    return b\n');
  });

  it('given a middle python statement that follows no block header, when it is indented by one unit, then it is would-not-parse syntax and nothing is applied', async () => {
    const text = 'def f(x):\n    a = 1\n    b = 2\n    c = 3\n';

    const reply = await previewOf(lezer, 'python', text, [reindent(between(text, 'b = 2'), 1)]);

    expect(outcomeOf(reply)).toMatchObject({ code: 'would-not-parse', reason: 'syntax' });
  });

  it('given a python statement after a loop, when it is indented by one unit, then it joins the loop body, which the text allows', async () => {
    const text = 'def f(xs):\n    for x in xs:\n        g(x)\n    h()\n';

    const reply = await previewOf(lezer, 'python', text, [reindent(between(text, 'h()'), 1)]);

    expect(outcomeOf(reply)).toBe('def f(xs):\n    for x in xs:\n        g(x)\n        h()\n');
  });

  it('given a python statement over two lines pasted at a shallow indent, when it is re-indented with no count, then the second line keeps its offset to the first', async () => {
    const text = 'def f(x):\n  total = g(1,\n          2)\n  return total\n';

    const reply = await previewOf(lezer, 'python', text, [reindent(between(text, 'total = g(1,', 'return total'))]);

    expect(outcomeOf(reply)).toBe('def f(x):\n    total = g(1,\n            2)\n    return total\n');
  });

  it('given a python block with a triple-quoted string, when it is re-indented with no count or by a count, then the inner lines of the string keep their text', async () => {
    const text = 'def f(x):\n  s = """\n inner\n      low"""\n  return s\n';

    const normal = await previewOf(lezer, 'python', text, [reindent(between(text, 's = """', 'return s'))]);
    const counted = await previewOf(lezer, 'python', text, [reindent(between(text, 's = """', 'return s'), 1)]);

    expect(outcomeOf(normal)).toBe('def f(x):\n    s = """\n inner\n      low"""\n    return s\n');
    expect(outcomeOf(counted)).toBe('def f(x):\n      s = """\n inner\n      low"""\n      return s\n');
  });

  it('given a tab-indented python block, when it is indented by one unit, then the tab prefix is kept and a unit of spaces is added', async () => {
    const text = 'def f(x):\n\ta = 1\n\tb = 2\n';

    const reply = await previewOf(lezer, 'python', text, [reindent(between(text, 'a = 1', 'b = 2'), 1)]);

    expect(outcomeOf(reply)).toBe('def f(x):\n\t    a = 1\n\t    b = 2\n');
  });

  it('given a block with an Allman brace on its own line, when it is re-indented with no count, then the opening and the closing brace take the depth of their owner', async () => {
    const text = 'function f(x)\n{\n      if (x)\n   {\n   a();\n      }\n}\n';

    const reply = await previewOf(lezer, 'javascript', text, [reindent(between(text, 'if (x)', '      }'))]);

    expect(outcomeOf(reply)).toBe('function f(x)\n{\n    if (x)\n    {\n        a();\n    }\n}\n');
  });

  it('given a braceless body, when it is re-indented, then it is not-a-node not-in-block naming it', async () => {
    const text = 'function f(x) {\n  if (x)\n    a = 1;\n}\n';

    const reply = await previewOf(lezer, 'javascript', text, [reindent(between(text, 'a = 1;'))]);

    expect(outcomeOf(reply)).toEqual({ code: 'not-a-node', reason: 'not-in-block', range: between(text, 'a = 1;') });
  });
});
