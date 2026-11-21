import { describe, expect, it } from 'vitest';

import { applyChanges, createEditorClient, planBatch, unwrap, type Range, type Step } from '../../../src';
import { locateRun, type RunInput } from '../../../src/core/semantic/model';
import { planMove } from '../../../src/core/semantic/ops/move';
import type { PlanContext } from '../../../src/core/semantic/plan';
import { verifyStructure } from '../../../src/core/semantic/verify';
import { createFakeEditor } from '../../../src/testing';
import { lezerSnapshot, realTree } from '../../support/lezerPlain';
import { between, outcomeOf, PARSERS, previewOf } from '../../support/semanticPlan';

type Destination = Extract<Step, { op: 'moveNode' }>['to'];
const move = (node: Range, to: Destination): Step => ({ op: 'moveNode', node, to });
const copy = (node: Range): Step => ({ op: 'duplicateNode', node });

const PY3 = 'def f(x):\n    if x:\n        a = 1\n        b = 2\n        c = 3\n    return b\n';
const JS3 = 'function f(x) {\n  if (x) {\n    a = 1;\n    b = 2;\n    c = 3;\n  }\n  return b;\n}\n';
const PY_NEST = 'def f(xs):\n    if xs:\n        a = 1\n    for x in xs:\n        g(x)\n';
const JS_NEST = 'function f(xs) {\n  if (xs) {\n    a = 1;\n  }\n  for (const x of xs) {\n    g(x);\n  }\n}\n';

describe.each(PARSERS)('moveNode planned over %s', (_name, parser) => {
  it('given a run of two statements in an if of three, when it moves after the if, then both lines move to the if indent, in python and in braces', async () => {
    const python = await previewOf(parser, 'python', PY3, [move(between(PY3, 'b = 2', 'c = 3'), { after: between(PY3, 'if x:', 'c = 3') })]);
    const braces = await previewOf(parser, 'javascript', JS3, [move(between(JS3, 'b = 2;', 'c = 3;'), { after: between(JS3, 'if (x) {', '  }') })]);

    expect(outcomeOf(python)).toBe('def f(x):\n    if x:\n        a = 1\n    b = 2\n    c = 3\n    return b\n');
    expect(outcomeOf(braces)).toBe('function f(x) {\n  if (x) {\n    a = 1;\n  }\n  b = 2;\n  c = 3;\n  return b;\n}\n');
  });

  it('given a compound statement with nested lines, when it moves into a loop, then every inner line stays one level inside it at the new depth', async () => {
    const python = await previewOf(parser, 'python', PY_NEST, [move(between(PY_NEST, 'if xs:', 'a = 1'), { into: between(PY_NEST, 'for x in xs:', 'g(x)'), at: 'end' })]);
    const braces = await previewOf(parser, 'javascript', JS_NEST, [move(between(JS_NEST, 'if (xs) {', '  }'), { into: between(JS_NEST, 'for (const x of xs) {', '    g(x);\n  }'), at: 'end' })]);

    expect(outcomeOf(python)).toBe('def f(xs):\n    for x in xs:\n        g(x)\n        if xs:\n            a = 1\n');
    expect(outcomeOf(braces)).toBe('function f(xs) {\n  for (const x of xs) {\n    g(x);\n    if (xs) {\n      a = 1;\n    }\n  }\n}\n');
  });

  it.each([
    ['after itself', (text: string) => ({ after: between(text, 'a = 1') })],
    ['before itself', (text: string) => ({ before: between(text, 'a = 1') })],
    ['before its next sibling', (text: string) => ({ before: between(text, 'b = 2') })],
  ] as const)('given a statement moved %s, when it is previewed, then it is ok and the text and the diff are empty', async (_where, destination) => {
    const reply = await previewOf(parser, 'python', PY3, [move(between(PY3, 'a = 1'), destination(PY3))]);

    expect(outcomeOf(reply)).toBe(PY3);
    expect(reply.ok && reply.value.preview?.diff).toEqual([]);
  });

  it('given a statement after its previous sibling, when it moves after it, then it is ok and nothing changes', async () => {
    const reply = await previewOf(parser, 'python', PY3, [move(between(PY3, 'b = 2'), { after: between(PY3, 'a = 1') })]);

    expect(outcomeOf(reply)).toBe(PY3);
  });

  it('given a blank line below the reference, when a statement moves after the reference, then it lands right under it and not after the blank line', async () => {
    const text = 'def f(x):\n    a = 1\n\n    b = 2\n    c = 3\n';

    const reply = await previewOf(parser, 'python', text, [move(between(text, 'c = 3'), { after: between(text, 'a = 1') })]);

    expect(outcomeOf(reply)).toBe('def f(x):\n    a = 1\n    c = 3\n\n    b = 2\n');
  });

  it('given a python if with an else, when a statement moves to the start of block 1, then it opens the else body, and block 2 is not-in-block naming the compound', async () => {
    const text = 'def f(x):\n    if x:\n        a = 1\n    else:\n        b = 2\n    return 1\n';
    const compound = between(text, 'if x:', 'b = 2');

    const first = await previewOf(parser, 'python', text, [move(between(text, 'return 1'), { into: compound, at: 'start', block: 1 })]);
    const third = await previewOf(parser, 'python', text, [move(between(text, 'return 1'), { into: compound, at: 'start', block: 2 })]);

    expect(outcomeOf(first)).toBe('def f(x):\n    if x:\n        a = 1\n    else:\n        return 1\n        b = 2\n');
    expect(outcomeOf(third)).toEqual({ code: 'not-a-node', reason: 'not-in-block', range: compound });
  });

  it.each([
    ['braces on one line', 'function f(x) {\n    if (x) {}\n    g();\n}\n', 'if (x) {}'],
    ['braces opened across two lines', 'function f(x) {\n    if (x) {\n    }\n    g();\n}\n', 'if (x) {\n    }'],
  ])('given an empty block with %s, when a statement moves into it, then the block opens at the owner indent plus one unit', async (_shape, text, compound) => {
    const reply = await previewOf(parser, 'javascript', text, [move(between(text, 'g();'), { into: between(text, compound), at: 'end' })]);

    expect(outcomeOf(reply)).toBe('function f(x) {\n    if (x) {\n        g();\n    }\n}\n');
  });

  it('given the only statement of a braces block, when it moves out and back, then the block is left empty and takes the statement again', async () => {
    const text = 'function f(x) {\n    if (x) {\n        g();\n    }\n}\n';
    const out = await previewOf(parser, 'javascript', text, [move(between(text, 'g();'), { after: between(text, 'if (x) {', '    }') })]);
    const emptied = 'function f(x) {\n    if (x) {\n    }\n    g();\n}\n';

    const back = await previewOf(parser, 'javascript', emptied, [move(between(emptied, 'g();'), { into: between(emptied, 'if (x) {', '    }'), at: 'start' })]);

    expect(outcomeOf(out)).toBe(emptied);
    expect(outcomeOf(back)).toBe(text);
  });

  it('given a comment line above the moved run and another above the reference, when the run moves before the reference, then the run takes its comment and lands above the reference comment', async () => {
    const text = 'def f(x):\n    # the first\n    a = 1\n    # the last\n    b = 2\n';

    const reply = await previewOf(parser, 'python', text, [move(between(text, 'b = 2'), { before: between(text, 'a = 1') })]);

    expect(outcomeOf(reply)).toBe('def f(x):\n    # the last\n    b = 2\n    # the first\n    a = 1\n');
  });

  it('given a selection inside the moved statement, when it moves, then the selection goes with the text; and a caret at the start of the line after it stays where that line goes', async () => {
    const fake = createFakeEditor({ text: PY3, tree: parser });
    const client = createEditorClient(fake.engine);
    unwrap(await client.select(PY3.indexOf('b = 2') + 1, PY3.indexOf('b = 2') + 4));

    unwrap(await client.moveNode(between(PY3, 'b = 2'), { after: between(PY3, 'if x:', 'c = 3') }));

    const moved = fake.text().indexOf('b = 2');
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: moved + 1, head: moved + 4 });
    unwrap(await client.undo());
    unwrap(await client.select(PY3.indexOf('        c = 3')));
    unwrap(await client.moveNode(between(PY3, 'b = 2'), { before: between(PY3, 'a = 1') }));
    expect(fake.peek().selection.ranges[0]?.head).toBe(fake.text().indexOf('        c = 3'));
  });
});

describe('moveNode planned over the installed grammars', () => {
  const lezer = lezerSnapshot;
  const JS_ELSE = 'function f(x) {\n  if (x) {\n    a = 1;\n  } else if (y) {\n    b = 2;\n  } else {\n    c = 3;\n  }\n  return 1;\n}\n';
  const JS_BARE = 'function f(x) {\n  if (x)\n    a = 1;\n  return 1;\n}\n';

  it('given the block of an else-if named by its range, when a statement moves to the end of block 1, then it lands in the final else', async () => {
    const reply = await previewOf(lezer, 'javascript', JS_ELSE, [move(between(JS_ELSE, 'return 1;'), { into: between(JS_ELSE, 'if (y) {', '    c = 3;\n  }'), at: 'end', block: 1 })]);

    expect(outcomeOf(reply)).toBe('function f(x) {\n  if (x) {\n    a = 1;\n  } else if (y) {\n    b = 2;\n  } else {\n    c = 3;\n    return 1;\n  }\n}\n');
  });

  it.each([
    ['an else-if as the reference', JS_ELSE, 'return 1;', (text: string): Destination => ({ before: between(text, 'if (y) {', '    c = 3;\n  }') })],
    ['a braceless body as the reference', JS_BARE, 'return 1;', (text: string): Destination => ({ before: between(text, 'a = 1;') })],
  ])('given %s, when a statement moves there, then it is not-a-node not-in-block naming the destination', async (_what, text, node, destination) => {
    const to = destination(text);

    const reply = await previewOf(lezer, 'javascript', text, [move(between(text, node), to)]);

    expect(outcomeOf(reply)).toEqual({ code: 'not-a-node', reason: 'not-in-block', range: 'before' in to ? to.before : undefined });
  });

  it.each([
    ['a braceless if', 'javascript', JS_BARE, 'if (x)\n    a = 1;', 'not-in-block'],
    ['a simple statement', 'javascript', JS_BARE, 'return 1;', 'no-statement'],
    ['a C++ switch', 'cpp', 'int f(int x) {\n  switch (x) {\n    case 1:\n      a = 1;\n      break;\n  }\n  return 1;\n}\n', 'switch (x) {\n    case 1:\n      a = 1;\n      break;\n  }', 'not-in-block'],
  ] as const)('given %s as the compound to move into, when a statement moves there, then it is not-a-node %s naming it', async (_what, language, text, into, reason) => {
    const range = between(text, into);

    const reply = await previewOf(lezer, language, text, [move(between(text, 'return 1;'), { into: range, at: 'end' })]);

    expect(outcomeOf(reply)).toEqual({ code: 'not-a-node', reason, range });
  });

  it('given a triple-quoted string in a python block, when the block moves one level deeper, then the inner lines of the string keep their text and its other lines move', async () => {
    const text = 'def f(x):\n    if x:\n        a = 1\n    s = """\n    inner\n  low"""\n    return s\n';

    const reply = await previewOf(lezer, 'python', text, [move(between(text, 's = """', 'low"""'), { into: between(text, 'if x:', 'a = 1'), at: 'end' })]);

    expect(outcomeOf(reply)).toBe('def f(x):\n    if x:\n        a = 1\n        s = """\n    inner\n  low"""\n    return s\n');
  });

  it('given a tab-indented python file, when a statement with a hanging continuation moves one block deeper, then tabs stay tabs and the continuation keeps its offset', async () => {
    const text = 'def f(x):\n\tif x:\n\t\ta = 1\n\treturn (1,\n\t\t2)\n';

    const reply = await previewOf(lezer, 'python', text, [move(between(text, 'return (1,', '2)'), { into: between(text, 'if x:', 'a = 1'), at: 'end' })]);

    expect(outcomeOf(reply)).toBe('def f(x):\n\tif x:\n\t\ta = 1\n\t\treturn (1,\n\t\t\t2)\n');
  });

  it('given a python statement moved into a block that its text cannot join, when the planner resolves the destination one level too deep, then the verifier refuses it as would-not-parse structure', () => {
    const text = 'def f(xs):\n    for x in xs:\n        g(x)\n    h()\n    return 1\n';
    const input: RunInput = { text, root: realTree('python', text), language: 'python' };
    const reference = between(text, 'for x in xs:', 'g(x)');
    const context: PlanContext = {
      step: 0,
      input,
      indentUnit: '    ',
      locate: (target) => {
        const located = locateRun(input, target as Range);
        if (!located.ok) return located;
        const mistaken = JSON.stringify(target) === JSON.stringify(reference);
        return { ok: true, value: mistaken ? { ...located.value, indentPrefix: `${located.value.indentPrefix}    ` } : located.value };
      },
    };

    const planned = planMove(move(between(text, 'return 1'), { after: reference }) as Extract<Step, { op: 'moveNode' }>, context);

    if (!planned.ok) throw new Error(planned.error.message);
    const batch = planBatch({ length: text.length, slice: (from, to) => text.slice(from, to) }, [{ op: 'splice', edits: planned.value.edits, landed: planned.value.landed }]);
    if (!batch.ok) throw new Error(batch.error.message);
    const result = applyChanges(text, batch.plan.changes);
    expect(result).toBe('def f(xs):\n    for x in xs:\n        g(x)\n        return 1\n    h()\n');
    const verified = verifyStructure({ language: 'python', base: input.root, result: realTree('python', result), changes: batch.plan.changes, written: batch.plan.written, expectations: [planned.value.expectation] });
    expect(verified).toMatchObject({ ok: false, error: { code: 'would-not-parse', reason: 'structure' } });
  });
});

describe.each(PARSERS)('duplicateNode planned over %s', (_name, parser) => {
  it('given a statement under a comment line, when it is duplicated, then the copy takes the comment and follows the statement at its indent', async () => {
    const text = 'def f(x):\n    if x:\n        # note\n        a = 1\n    return b\n';

    const reply = await previewOf(parser, 'python', text, [copy(between(text, 'a = 1'))]);

    expect(outcomeOf(reply)).toBe('def f(x):\n    if x:\n        # note\n        a = 1\n        # note\n        a = 1\n    return b\n');
  });

  it('given a run of two statements in braces, when it is duplicated, then both lines are copied after the run', async () => {
    const reply = await previewOf(parser, 'javascript', JS3, [copy(between(JS3, 'b = 2;', 'c = 3;'))]);

    expect(outcomeOf(reply)).toBe('function f(x) {\n  if (x) {\n    a = 1;\n    b = 2;\n    c = 3;\n    b = 2;\n    c = 3;\n  }\n  return b;\n}\n');
  });

  it('given a duplicated python compound with a nested block, when it is previewed, then the copy keeps every inner line at its offset', async () => {
    const reply = await previewOf(parser, 'python', PY_NEST, [copy(between(PY_NEST, 'if xs:', 'a = 1'))]);

    expect(outcomeOf(reply)).toBe('def f(xs):\n    if xs:\n        a = 1\n    if xs:\n        a = 1\n    for x in xs:\n        g(x)\n');
  });
});

describe('duplicateNode planned over the installed grammars', () => {
  it('given a braceless body, when it is duplicated, then it is not-a-node not-in-block naming it', async () => {
    const text = 'function f(x) {\n  if (x)\n    a = 1;\n  return 1;\n}\n';

    const reply = await previewOf(lezerSnapshot, 'javascript', text, [copy(between(text, 'a = 1;'))]);

    expect(outcomeOf(reply)).toEqual({ code: 'not-a-node', reason: 'not-in-block', range: between(text, 'a = 1;') });
  });
});
