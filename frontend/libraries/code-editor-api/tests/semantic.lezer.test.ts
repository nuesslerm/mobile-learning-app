import { describe, expect, it } from 'vitest';

import { createEditorClient, type ApplyValue, type Language, type Range, type Reply, type Step } from '../src';
import { createFakeEditor } from '../src/testing';
import { lezerSnapshot } from './support/lezerPlain';

// What deleteNode plans over the fake editor's tiny parsers and over the installed grammars, asked as a preview so nothing is applied.
async function planned(language: Language, text: string, range: Range, parser: 'fake' | 'lezer'): Promise<Reply<ApplyValue>> {
  const client = createEditorClient(createFakeEditor({ text, tree: parser === 'lezer' ? lezerSnapshot : true }).engine);
  await client.setConfig({ language });
  return client.deleteNode(range, { preview: true });
}

async function plannedSteps(language: Language, text: string, steps: readonly Step[], parser: 'fake' | 'lezer'): Promise<Reply<ApplyValue>> {
  const client = createEditorClient(createFakeEditor({ text, tree: parser === 'lezer' ? lezerSnapshot : true }).engine);
  await client.setConfig({ language });
  return client.transact(steps, { preview: true });
}

const between = (text: string, first: string, last: string): Range => ({ from: text.indexOf(first), to: text.indexOf(last) + last.length });
const lineOf = (text: string, needle: string): Range => ({ from: text.indexOf(needle), to: text.indexOf(needle) + needle.length });

const outcome = (reply: Reply<ApplyValue>) => (reply.ok ? { text: reply.value.preview?.text, diff: reply.value.preview?.diff } : { code: reply.error.code, reason: 'reason' in reply.error ? reply.error.reason : undefined });

type Case = { readonly name: string; readonly language: Language; readonly text: string; readonly range: Range; readonly result: 'applies' | 'would-not-parse' };

const BRANCH = {
  python: 'def f(x):\n    if x:\n        a = 1\n        b = 2\n    return b\n',
  javascript: 'function f(x) {\n  if (x) {\n    a = 1;\n    b = 2;\n  }\n  return b;\n}\n',
  cpp: 'int f(int x) {\n  if (x) {\n    a = 1;\n    b = 2;\n  }\n  return b;\n}\n',
} as const;
const SECOND = { python: 'b = 2', javascript: 'b = 2;', cpp: 'b = 2;' } as const;
const FIRST = { python: 'a = 1', javascript: 'a = 1;', cpp: 'a = 1;' } as const;
const IF = { python: ['if x:', 'b = 2'], javascript: ['if (x) {', '  }'], cpp: ['if (x) {', '  }'] } as const;
const COMMENTED = {
  python: 'def f(x):\n    if x:\n        # the first\n        a = 1\n        b = 2\n    return b\n',
  javascript: 'function f(x) {\n  if (x) {\n    // the first\n    a = 1;\n    b = 2;\n  }\n  return b;\n}\n',
  cpp: 'int f(int x) {\n  if (x) {\n    // the first\n    a = 1;\n    b = 2;\n  }\n  return b;\n}\n',
} as const;

const LANGUAGES: readonly Language[] = ['python', 'javascript', 'cpp'];
const CASES: Case[] = LANGUAGES.flatMap((language): Case[] => {
  const text = BRANCH[language];
  const commented = COMMENTED[language];
  return [
    { name: 'the second statement of a block of two', language, text, range: lineOf(text, SECOND[language]), result: 'applies' },
    { name: 'a run of both statements of a block', language, text, range: between(text, FIRST[language], SECOND[language]), result: language === 'python' ? 'would-not-parse' : 'applies' },
    { name: 'a whole if with its block', language, text, range: between(text, IF[language][0], language === 'python' ? 'b = 2' : '  }'), result: 'applies' },
    { name: 'the statement under a comment line', language, text: commented, range: lineOf(commented, FIRST[language]), result: 'applies' },
  ];
});

describe('deleteNode planned over the fake parsers and over the installed grammars', () => {
  it.each(CASES)('given $name in $language, when it is previewed, then both parsers plan the same edit or the same refusal', async ({ language, text, range, result }) => {
    const fake = outcome(await planned(language, text, range, 'fake'));
    const lezer = outcome(await planned(language, text, range, 'lezer'));

    expect(lezer).toEqual(fake);
    expect('text' in lezer ? 'applies' : lezer.code).toBe(result);
  });

  it.each(LANGUAGES)('given a statement under a comment line in %s, when it is deleted, then the comment goes with it', async (language) => {
    const text = COMMENTED[language];

    const reply = await planned(language, text, lineOf(text, FIRST[language]), 'lezer');

    expect(reply).toMatchObject({ ok: true, value: { preview: { text: text.replace(/ *(#|\/\/) the first\n *a = 1;?\n/, '') } } });
  });
});

const ROOMS = {
  python: 'def f(x):\n    if x:\n        a = 1\n        b = 2\n        c = 3\n    return b\n',
  javascript: 'function f(x) {\n  if (x) {\n    a = 1;\n    b = 2;\n    c = 3;\n  }\n  return b;\n}\n',
  cpp: 'int f(int x) {\n  if (x) {\n    a = 1;\n    b = 2;\n    c = 3;\n  }\n  return b;\n}\n',
} as const;
const THIRD = { python: 'c = 3', javascript: 'c = 3;', cpp: 'c = 3;' } as const;
const RETURN = { python: 'return b', javascript: 'return b;', cpp: 'return b;' } as const;
const IF_END = { python: 'c = 3', javascript: '  }', cpp: '  }' } as const;
const EMPTY = {
  javascript: 'function f(x) {\n    if (x) {}\n    g();\n}\n',
  cpp: 'int f(int x) {\n    if (x) {}\n    g();\n}\n',
} as const;

describe('moveNode and duplicateNode planned over the fake parsers and over the installed grammars', () => {
  const steps = (language: Language, text: string): { readonly name: string; readonly steps: Step[] }[] => {
    const ifNode = between(text, IF[language][0], IF_END[language]);
    return [
      { name: 'the last statement of the if moved after the if', steps: [{ op: 'moveNode', node: lineOf(text, THIRD[language]), to: { after: ifNode } }] },
      { name: 'a run of two statements moved after the if', steps: [{ op: 'moveNode', node: between(text, SECOND[language], THIRD[language]), to: { after: ifNode } }] },
      { name: 'the statement after the if moved to the start of its block', steps: [{ op: 'moveNode', node: lineOf(text, RETURN[language]), to: { into: ifNode, at: 'start' } }] },
      { name: 'the first statement moved before its next sibling', steps: [{ op: 'moveNode', node: lineOf(text, FIRST[language]), to: { before: lineOf(text, SECOND[language]) } }] },
      { name: 'the whole if moved after the statement below it', steps: [{ op: 'moveNode', node: ifNode, to: { after: lineOf(text, RETURN[language]) } }] },
      { name: 'the if moved into itself', steps: [{ op: 'moveNode', node: ifNode, to: { into: ifNode, at: 'end' } }] },
      { name: 'the second statement duplicated', steps: [{ op: 'duplicateNode', node: lineOf(text, SECOND[language]) }] },
      { name: 'the whole if duplicated', steps: [{ op: 'duplicateNode', node: ifNode }] },
    ];
  };
  const CASES = LANGUAGES.flatMap((language) => steps(language, ROOMS[language]).map((one) => ({ ...one, language, text: ROOMS[language] })));

  it.each(CASES)('given $name in $language, when it is previewed, then both parsers plan the same edit or the same refusal', async ({ language, text, steps: asked }) => {
    const fake = outcome(await plannedSteps(language, text, asked, 'fake'));
    const lezer = outcome(await plannedSteps(language, text, asked, 'lezer'));

    expect(lezer).toEqual(fake);
  });

  it.each(['javascript', 'cpp'] as const)('given an empty block in %s, when a statement moves into it, then both parsers open the braces the same way', async (language) => {
    const text = EMPTY[language];
    const asked: Step[] = [{ op: 'moveNode', node: lineOf(text, 'g();'), to: { into: lineOf(text, 'if (x) {}'), at: 'end' } }];

    const lezer = outcome(await plannedSteps(language, text, asked, 'lezer'));

    expect(lezer).toEqual(outcome(await plannedSteps(language, text, asked, 'fake')));
    expect(lezer).toMatchObject({ text: text.includes('int') ? 'int f(int x) {\n    if (x) {\n        g();\n    }\n}\n' : 'function f(x) {\n    if (x) {\n        g();\n    }\n}\n' });
  });
});

describe('deleteNode on the installed grammars', () => {
  it('given two Python statements on one line, when the second is deleted, then it is not-a-node because they share their line', async () => {
    const text = 'def f(x):\n    a = 1; b = 2\n    return b\n';

    const reply = await planned('python', text, lineOf(text, 'b = 2'), 'lezer');

    expect(reply).toMatchObject({ ok: false, error: { code: 'not-a-node', reason: 'shares-line' } });
  });

  it.each([
    ['javascript', 'function f(x) {\n  a = 1; b = 2;\n  return b;\n}\n', 'b = 2;'],
    ['cpp', 'int f(int x) {\n  a = 1; b = 2;\n  return b;\n}\n', 'b = 2;'],
  ] as const)('given two %s statements on one line, when the second is deleted, then it is not-a-node because they share their line', async (language, text, second) => {
    const reply = await planned(language, text, lineOf(text, second), 'lezer');

    expect(reply).toMatchObject({ ok: false, error: { code: 'not-a-node', reason: 'shares-line' } });
  });

  it.each([
    ['javascript', 'function f(x) {\n  if (x)\n    a = 1;\n  return a;\n}\n', 'a = 1;'],
    ['cpp', 'int f(int x) {\n  if (x)\n    a = 1;\n  return a;\n}\n', 'a = 1;'],
  ] as const)('given the braceless body of an if in %s, when it is deleted, then it is not-a-node because it is not in a block', async (language, text, body) => {
    const reply = await planned(language, text, lineOf(text, body), 'lezer');

    expect(reply).toMatchObject({ ok: false, error: { code: 'not-a-node', reason: 'not-in-block' } });
  });

  it.each([
    ['javascript', 'function f(x) {\n  if (x) {\n    a = 1;\n  } else if (y) {\n    b = 2;\n  }\n}\n'],
    ['cpp', 'int f(int x) {\n  if (x) {\n    a = 1;\n  } else if (y) {\n    b = 2;\n  }\n}\n'],
  ] as const)('given the else-if of an if in %s, when it is deleted, then it is not-a-node because it is not in a block', async (language, text) => {
    const reply = await planned(language, text, between(text, 'if (y) {', '    b = 2;\n  }'), 'lezer');

    expect(reply).toMatchObject({ ok: false, error: { code: 'not-a-node', reason: 'not-in-block' } });
  });

  it('given a Python statement with a syntax error, when it is deleted, then it is not-a-node because it is damaged', async () => {
    const text = 'def f(x):\n    for i in\n    return x\n';

    const reply = await planned('python', text, lineOf(text, 'for i in'), 'lezer');

    expect(reply).toMatchObject({ ok: false, error: { code: 'not-a-node', reason: 'damaged' } });
  });

  it.each([
    ['python', 'def f():\n    s = """\n    # inside"""\n    return s\n', 'return s', 'def f():\n    s = """\n    # inside"""\n'],
    ['javascript', 'function f() {\n  const s = `\n  // inside`;\n  return s;\n}\n', 'return s;', 'function f() {\n  const s = `\n  // inside`;\n}\n'],
    ['cpp', 'int f() {\n  auto s = R"(\n  // inside)";\n  return 0;\n}\n', 'return 0;', 'int f() {\n  auto s = R"(\n  // inside)";\n}\n'],
  ] as const)('given a %s statement under the last line of a multi-line string that looks like a comment, when it is deleted, then only its own line goes', async (language, text, statement, expected) => {
    const reply = await planned(language, text, lineOf(text, statement), 'lezer');

    expect(reply).toMatchObject({ ok: true, value: { preview: { text: expected } } });
  });

  it('given a multi-line Python string statement, when it is deleted, then all its lines go', async () => {
    const text = 'def f():\n    s = """\n    # inside"""\n    return 1\n';

    const reply = await planned('python', text, between(text, 's = """', '# inside"""'), 'lezer');

    expect(reply).toMatchObject({ ok: true, value: { preview: { text: 'def f():\n    return 1\n' } } });
  });
});
