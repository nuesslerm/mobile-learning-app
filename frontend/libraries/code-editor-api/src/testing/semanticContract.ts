import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { createEditorClient, unwrap, type EditorClient } from '../core/client';
import { isSemanticOp } from '../core/validate';
import { SEMANTIC_STEP_OPS, type EditorError, type EditorEvent, type Language, type Range, type Reply, type SemanticStepOp, type Step } from '../ports/protocol';
import type { MakeSubject, Subject } from './contract';

type Snippet = {
  /** a function with an if holding two statements, and a statement after the if */
  readonly branch: string;
  readonly second: string;
  /** a function with a loop holding one call, and a statement after the loop */
  readonly loop: string;
  readonly inner: string;
  /** a function whose last line has no newline */
  readonly bare: string;
  /** a function with two statements on one line */
  readonly shared: string;
  /** a function with a syntax error in it, then the branch function */
  readonly broken: string;
  /** the if statement of the branch, from its keyword to its last line, and the statement that opens its block */
  readonly ifNode: string;
  readonly first: string;
  /** the statement after the if, and the branch with the second statement moved after the if, and with the statement after the if moved to the end of its block */
  readonly last: string;
  readonly secondAfterIf: string;
  readonly lastIntoIf: string;
  /** an if holding one statement, a comment line and a statement after it at the if's indent, a statement after that; the if; the same with the commented statement at the start of the if */
  readonly noted: string;
  readonly notedIf: string;
  readonly notedStatement: string;
  readonly notedMoved: string;
  /** the loop's return statement, and the loop with its call moved after it */
  readonly ret: string;
  readonly loopEmptied: string;
  /** two statements at the top of the document, the last without a newline */
  readonly flat: string;
  readonly flatFirst: string;
  readonly flatLast: string;
};

const SNIPPETS: Readonly<Record<Language, Snippet>> = {
  python: {
    branch: 'def f(x):\n    if x:\n        a = 1\n        b = 2\n    return b\n',
    second: 'b = 2',
    loop: 'def f(xs):\n    for x in xs:\n        print(x)\n    return 1\n',
    inner: 'print(x)',
    bare: 'def f(x):\n    return x',
    shared: 'def f(x):\n    a = 1; b = 2\n    return b\n',
    broken: 'def g(x):\n    for i in\n    return x\n\ndef f(x):\n    if x:\n        a = 1\n        b = 2\n    return b\n',
    ifNode: 'if x:\n        a = 1\n        b = 2',
    first: 'a = 1',
    last: 'return b',
    secondAfterIf: 'def f(x):\n    if x:\n        a = 1\n    b = 2\n    return b\n',
    lastIntoIf: 'def f(x):\n    if x:\n        a = 1\n        b = 2\n        return b\n',
    noted: 'def f(x):\n    if x:\n        a = 1\n    # note\n    b = 2\n    return b\n',
    notedIf: 'if x:\n        a = 1',
    notedStatement: 'b = 2',
    notedMoved: 'def f(x):\n    if x:\n        # note\n        b = 2\n        a = 1\n    return b\n',
    ret: 'return 1',
    loopEmptied: 'def f(xs):\n    for x in xs:\n    return 1\n    print(x)\n',
    flat: 'a = 1\nb = 2',
    flatFirst: 'a = 1',
    flatLast: 'b = 2',
  },
  javascript: {
    branch: 'function f(x) {\n  if (x) {\n    a = 1;\n    b = 2;\n  }\n  return b;\n}\n',
    second: 'b = 2;',
    loop: 'function f(xs) {\n  for (const x of xs) {\n    g(x);\n  }\n  return 1;\n}\n',
    inner: 'g(x);',
    bare: 'function f(x) {\n  return x;\n}',
    shared: 'function f(x) {\n  a = 1; b = 2;\n  return b;\n}\n',
    broken: 'function g() {\n  let a = ;\n}\n\nfunction f(x) {\n  if (x) {\n    a = 1;\n    b = 2;\n  }\n  return b;\n}\n',
    ifNode: 'if (x) {\n    a = 1;\n    b = 2;\n  }',
    first: 'a = 1;',
    last: 'return b;',
    secondAfterIf: 'function f(x) {\n  if (x) {\n    a = 1;\n  }\n  b = 2;\n  return b;\n}\n',
    lastIntoIf: 'function f(x) {\n  if (x) {\n    a = 1;\n    b = 2;\n    return b;\n  }\n}\n',
    noted: 'function f(x) {\n  if (x) {\n    a = 1;\n  }\n  // note\n  b = 2;\n  return b;\n}\n',
    notedIf: 'if (x) {\n    a = 1;\n  }',
    notedStatement: 'b = 2;',
    notedMoved: 'function f(x) {\n  if (x) {\n    // note\n    b = 2;\n    a = 1;\n  }\n  return b;\n}\n',
    ret: 'return 1;',
    loopEmptied: 'function f(xs) {\n  for (const x of xs) {\n  }\n  return 1;\n  g(x);\n}\n',
    flat: 'let a = 1;\nlet b = 2;',
    flatFirst: 'let a = 1;',
    flatLast: 'let b = 2;',
  },
  cpp: {
    branch: 'int f(int x) {\n  if (x) {\n    a = 1;\n    b = 2;\n  }\n  return b;\n}\n',
    second: 'b = 2;',
    loop: 'int f() {\n  for (int i = 0; i < 3; i++) {\n    g(i);\n  }\n  return 1;\n}\n',
    inner: 'g(i);',
    bare: 'int f() {\n  return 1;\n}',
    shared: 'int f(int x) {\n  a = 1; b = 2;\n  return b;\n}\n',
    broken: 'int g() {\n  int a = ;\n}\n\nint f(int x) {\n  if (x) {\n    a = 1;\n    b = 2;\n  }\n  return b;\n}\n',
    ifNode: 'if (x) {\n    a = 1;\n    b = 2;\n  }',
    first: 'a = 1;',
    last: 'return b;',
    secondAfterIf: 'int f(int x) {\n  if (x) {\n    a = 1;\n  }\n  b = 2;\n  return b;\n}\n',
    lastIntoIf: 'int f(int x) {\n  if (x) {\n    a = 1;\n    b = 2;\n    return b;\n  }\n}\n',
    noted: 'int f(int x) {\n  if (x) {\n    a = 1;\n  }\n  // note\n  b = 2;\n  return b;\n}\n',
    notedIf: 'if (x) {\n    a = 1;\n  }',
    notedStatement: 'b = 2;',
    notedMoved: 'int f(int x) {\n  if (x) {\n    // note\n    b = 2;\n    a = 1;\n  }\n  return b;\n}\n',
    ret: 'return 1;',
    loopEmptied: 'int f() {\n  for (int i = 0; i < 3; i++) {\n  }\n  return 1;\n  g(i);\n}\n',
    flat: 'int a = 1;\nint b = 2;',
    flatFirst: 'int a = 1;',
    flatLast: 'int b = 2;',
  },
};

type WrapSnippet = {
  /** a header that makes a loop, one that does not parse, and one whose lines the grammar puts in a bare block (or, in Python, that does not parse either) */
  readonly header: string;
  readonly broken: string;
  readonly trap: string;
  /** the branch with its two statements under the loop header */
  readonly wrapped: string;
  /** a function with a statement over two lines, then a statement; the two-line statement; the function with it under the loop header */
  readonly hang: string;
  readonly hangNode: string;
  readonly hangWrapped: string;
  /** the loop statement of `loop`, and `loop` with it unwrapped */
  readonly loopNode: string;
  readonly unwrapped: string;
  /** an if with an else, then a statement; the if */
  readonly ifElse: string;
  readonly ifElseNode: string;
  /** a document of two statements with no final newline, the last one, and the document with the last one under the loop header */
  readonly tail: string;
  readonly tailNode: string;
  readonly tailWrapped: string;
};

const WRAPS: Readonly<Record<Language, WrapSnippet>> = {
  python: {
    header: 'for i in range(2)',
    broken: 'for i in',
    trap: 'if x: foo()',
    wrapped: 'def f(x):\n    if x:\n        for i in range(2):\n            a = 1\n            b = 2\n    return b\n',
    hang: 'def f(x):\n    total = g(1,\n            2)\n    return total\n',
    hangNode: 'total = g(1,\n            2)',
    hangWrapped: 'def f(x):\n    for i in range(2):\n        total = g(1,\n                2)\n    return total\n',
    loopNode: 'for x in xs:\n        print(x)',
    unwrapped: 'def f(xs):\n    print(x)\n    return 1\n',
    ifElse: 'def f(x):\n    if x:\n        a = 1\n    else:\n        b = 2\n    return 1\n',
    ifElseNode: 'if x:\n        a = 1\n    else:\n        b = 2',
    tail: 'a = 1\nb = 2',
    tailNode: 'b = 2',
    tailWrapped: 'a = 1\nfor i in range(2):\n    b = 2',
  },
  javascript: {
    header: 'for (let i = 0; i < 2; i++)',
    broken: 'for (',
    trap: 'if (x) foo();',
    wrapped: 'function f(x) {\n  if (x) {\n    for (let i = 0; i < 2; i++) {\n        a = 1;\n        b = 2;\n    }\n  }\n  return b;\n}\n',
    hang: 'function f(x) {\n  total = g(1,\n          2);\n  return total;\n}\n',
    hangNode: 'total = g(1,\n          2);',
    hangWrapped: 'function f(x) {\n  for (let i = 0; i < 2; i++) {\n      total = g(1,\n              2);\n  }\n  return total;\n}\n',
    loopNode: 'for (const x of xs) {\n    g(x);\n  }',
    unwrapped: 'function f(xs) {\n  g(x);\n  return 1;\n}\n',
    ifElse: 'function f(x) {\n  if (x) {\n    a = 1;\n  } else {\n    b = 2;\n  }\n  return 1;\n}\n',
    ifElseNode: 'if (x) {\n    a = 1;\n  } else {\n    b = 2;\n  }',
    tail: 'let a = 1;\nlet b = 2;',
    tailNode: 'let b = 2;',
    tailWrapped: 'let a = 1;\nfor (let i = 0; i < 2; i++) {\n    let b = 2;\n}',
  },
  cpp: {
    header: 'for (int i = 0; i < 2; i++)',
    broken: 'for (',
    trap: 'if (x) foo();',
    wrapped: 'int f(int x) {\n  if (x) {\n    for (int i = 0; i < 2; i++) {\n        a = 1;\n        b = 2;\n    }\n  }\n  return b;\n}\n',
    hang: 'int f(int x) {\n  total = g(1,\n          2);\n  return total;\n}\n',
    hangNode: 'total = g(1,\n          2);',
    hangWrapped: 'int f(int x) {\n  for (int i = 0; i < 2; i++) {\n      total = g(1,\n              2);\n  }\n  return total;\n}\n',
    loopNode: 'for (int i = 0; i < 3; i++) {\n    g(i);\n  }',
    unwrapped: 'int f() {\n  g(i);\n  return 1;\n}\n',
    ifElse: 'int f(int x) {\n  if (x) {\n    a = 1;\n  } else {\n    b = 2;\n  }\n  return 1;\n}\n',
    ifElseNode: 'if (x) {\n    a = 1;\n  } else {\n    b = 2;\n  }',
    tail: 'int a = 1;\nint b = 2;',
    tailNode: 'int b = 2;',
    tailWrapped: 'int a = 1;\nfor (int i = 0; i < 2; i++) {\n    int b = 2;\n}',
  },
};

type EditSnippet = {
  /** the branch in four-space indentation: the base of the cases below */
  readonly even: string;
  /** a snippet with a relative indent, and `even` with it after the second statement */
  readonly snippet: string;
  readonly snippetFirst: string;
  readonly inserted: string;
  /** a snippet that is a clause of the if before it */
  readonly clause: string;
  /** `even` with the if pasted with a two-space body (and, in braces, a closing brace at the wrong depth), and that if */
  readonly pasted: string;
  readonly pastedIf: string;
  /** `even` with the second statement outdented one unit */
  readonly outdented: string;
  /** a function whose body of two statements is indented by two and has a statement over two lines, and the same at four */
  readonly hang: string;
  readonly hangFixed: string;
};

const EDITS: Readonly<Record<Language, EditSnippet>> = {
  python: {
    even: 'def f(x):\n    if x:\n        a = 1\n        b = 2\n    return b\n',
    snippet: 'if y:\n    c = 3',
    snippetFirst: 'if y:',
    inserted: 'def f(x):\n    if x:\n        a = 1\n        b = 2\n        if y:\n            c = 3\n    return b\n',
    clause: 'else:\n    c = 3',
    pasted: 'def f(x):\n    if x:\n      a = 1\n      b = 2\n    return b\n',
    pastedIf: 'if x:\n      a = 1\n      b = 2',
    outdented: 'def f(x):\n    if x:\n        a = 1\n    b = 2\n    return b\n',
    hang: 'def f(x):\n  total = g(1,\n          2)\n  return total\n',
    hangFixed: 'def f(x):\n    total = g(1,\n            2)\n    return total\n',
  },
  javascript: {
    even: 'function f(x) {\n    if (x) {\n        a = 1;\n        b = 2;\n    }\n    return b;\n}\n',
    snippet: 'if (y) {\n  c = 3;\n}',
    snippetFirst: 'if (y) {',
    inserted: 'function f(x) {\n    if (x) {\n        a = 1;\n        b = 2;\n        if (y) {\n          c = 3;\n        }\n    }\n    return b;\n}\n',
    clause: 'else {\n  c = 3;\n}',
    pasted: 'function f(x) {\n    if (x) {\n      a = 1;\n      b = 2;\n  }\n    return b;\n}\n',
    pastedIf: 'if (x) {\n      a = 1;\n      b = 2;\n  }',
    outdented: 'function f(x) {\n    if (x) {\n        a = 1;\n    b = 2;\n    }\n    return b;\n}\n',
    hang: 'function f(x) {\n  total = g(1,\n          2);\n  return total;\n}\n',
    hangFixed: 'function f(x) {\n    total = g(1,\n            2);\n    return total;\n}\n',
  },
  cpp: {
    even: 'int f(int x) {\n    if (x) {\n        a = 1;\n        b = 2;\n    }\n    return b;\n}\n',
    snippet: 'if (y) {\n  c = 3;\n}',
    snippetFirst: 'if (y) {',
    inserted: 'int f(int x) {\n    if (x) {\n        a = 1;\n        b = 2;\n        if (y) {\n          c = 3;\n        }\n    }\n    return b;\n}\n',
    clause: 'else {\n  c = 3;\n}',
    pasted: 'int f(int x) {\n    if (x) {\n      a = 1;\n      b = 2;\n  }\n    return b;\n}\n',
    pastedIf: 'if (x) {\n      a = 1;\n      b = 2;\n  }',
    outdented: 'int f(int x) {\n    if (x) {\n        a = 1;\n    b = 2;\n    }\n    return b;\n}\n',
    hang: 'int f(int x) {\n  total = g(1,\n          2);\n  return total;\n}\n',
    hangFixed: 'int f(int x) {\n    total = g(1,\n            2);\n    return total;\n}\n',
  },
};

const failed = (reply: Reply<unknown>): EditorError => {
  if (reply.ok) throw new Error('the call was expected to fail');
  return reply.error;
};

const rangeOf = (text: string, needle: string): Range => {
  const from = text.indexOf(needle);
  if (from < 0) throw new Error(`the snippet has no ${needle}`);
  return { from, to: from + needle.length };
};

/** The text without the whole line that holds `needle`. */
const withoutLine = (text: string, needle: string): { readonly text: string; readonly at: number } => {
  const found = text.indexOf(needle);
  const from = text.lastIndexOf('\n', found) + 1;
  return { text: text.slice(0, from) + text.slice(text.indexOf('\n', found) + 1), at: from };
};

/** The whole lines that hold `needle`, newline included. */
const wholeLines = (text: string, needle: string): Range => {
  const found = text.indexOf(needle);
  const end = text.indexOf('\n', found + needle.length);
  return { from: text.lastIndexOf('\n', found) + 1, to: end < 0 ? text.length : end + 1 };
};

const sampleOf = (op: SemanticStepOp, node: Range): Step => {
  switch (op) {
    case 'moveNode':
      return { op, node, to: { after: node } };
    case 'wrapNode':
      return { op, node, header: 'for i in range(2)' };
    case 'insertSnippet':
      return { op, at: { before: node }, text: 'x' };
    case 'reindentBlock':
    case 'unwrapNode':
    case 'deleteNode':
    case 'duplicateNode':
      return { op, node };
  }
};

/**
 * What the node steps (`moveNode`, `wrapNode`, ...) must do on every editor behind the typed API, for one language. A separate suite from
 * `editorContract`, like `syntaxTreeContract`: a real page runs one language per init, and every case skips unless `hello` announces its op.
 */
export function semanticContract(name: string, make: MakeSubject, language: Language): void {
  const snippet = SNIPPETS[language];
  describe(`the node step contract (${language}) over ${name}`, () => {
    let subject: Subject;
    let client: EditorClient;
    const cleanups: (() => void)[] = [];

    beforeAll(async () => {
      subject = await make({ bound: true });
      client = createEditorClient(subject.engine);
    });
    afterAll(async () => {
      await subject.dispose();
    });
    beforeEach(async (ctx) => {
      if (!unwrap(await client.hello()).capabilities.languages.includes(language)) return ctx.skip();
      unwrap(await client.setConfig({ language, tabSize: 4 }));
    });
    afterEach(() => {
      for (const cleanup of cleanups.splice(0)) cleanup();
    });

    const start = async (text: string): Promise<number> => {
      const reply = await client.load(text);
      unwrap(reply);
      return reply.revision as number;
    };
    const snapshot = async () => {
      const reply = await client.getText();
      return { ...unwrap(reply), revision: reply.revision as number };
    };
    const announces = async (op: SemanticStepOp) => unwrap(await client.hello()).capabilities.steps.includes(op);
    const changes = () => {
      const seen: EditorEvent[] = [];
      cleanups.push(subject.engine.listen(['changed'], (event) => seen.push(event)));
      return seen;
    };

    describe('lifecycle', () => {
      it('given an engine that does not announce a node step, when it is sent, then it is unsupported naming step:<op> with the text and revision unchanged, and a step it announces needs the tree request', async (ctx) => {
        const hello = unwrap(await client.hello());
        const announced = hello.capabilities.steps.filter(isSemanticOp);
        expect(announced.length === 0 || hello.capabilities.requests.includes('tree')).toBe(true);
        const missing = SEMANTIC_STEP_OPS.filter((op) => !announced.includes(op));
        if (missing.length === 0) return ctx.skip();
        const base = await start(snippet.branch);

        for (const op of missing) {
          const reply = await client.transact([sampleOf(op, rangeOf(snippet.branch, snippet.second))]);

          expect(failed(reply)).toMatchObject({ code: 'unsupported', what: `step:${op}` });
        }

        expect(await snapshot()).toMatchObject({ text: snippet.branch, revision: base });
      });
    });

    describe('deleteNode', () => {
      const second = () => rangeOf(snippet.branch, snippet.second);
      const gone = () => withoutLine(snippet.branch, snippet.second);

      it('given a function with an if holding two statements, when the second is deleted, then the text loses exactly that line in one revision with one api change, and one undo restores it', async (ctx) => {
        if (!(await announces('deleteNode'))) return ctx.skip();
        const base = await start(snippet.branch);
        const seen = changes();

        const reply = await client.deleteNode(second());

        const value = unwrap(reply);
        expect(reply).toMatchObject({ revision: base + 1, base });
        expect(value.changes).toHaveLength(1);
        expect(value.landed).toEqual([{ from: gone().at, to: gone().at }]);
        expect(await snapshot()).toMatchObject({ text: gone().text, revision: base + 1 });
        expect(seen).toMatchObject([{ type: 'changed', from: base, to: base + 1, origin: 'api' }]);
        unwrap(await client.undo());
        expect(await snapshot()).toMatchObject({ text: snippet.branch });
      });

      it('given the only statement of a loop, when it is deleted, then Python refuses it as would-not-parse with the error in the would-be text and nothing changes, and braces leave an empty block', async (ctx) => {
        if (!(await announces('deleteNode'))) return ctx.skip();
        const base = await start(snippet.loop);
        const wouldBe = withoutLine(snippet.loop, snippet.inner).text;

        const reply = await client.deleteNode(rangeOf(snippet.loop, snippet.inner));

        if (language === 'python') {
          const error = failed(reply);
          expect(error).toMatchObject({ code: 'would-not-parse', reason: 'syntax' });
          if (error.code === 'would-not-parse') {
            expect(error.errors.length).toBeGreaterThan(0);
            expect(error.errors.every((range) => range.from >= 0 && range.to <= wouldBe.length)).toBe(true);
          }
          expect(await snapshot()).toMatchObject({ text: snippet.loop, revision: base });
          return;
        }
        unwrap(reply);
        expect(await snapshot()).toMatchObject({ text: wouldBe, revision: base + 1 });
      });

      it('given a range in the middle of an expression and two statements on one line, when they are deleted, then each is not-a-node and nothing changes', async (ctx) => {
        if (!(await announces('deleteNode'))) return ctx.skip();
        const base = await start(snippet.branch);

        expect(failed(await client.deleteNode(rangeOf(snippet.branch, '1')))).toMatchObject({ code: 'not-a-node', reason: 'no-statement', step: 0 });
        expect(await snapshot()).toMatchObject({ text: snippet.branch, revision: base });
        await start(snippet.shared);
        expect(failed(await client.deleteNode(rangeOf(snippet.shared, 'b = 2')))).toMatchObject({ code: 'not-a-node', step: 0 });
        expect(await snapshot()).toMatchObject({ text: snippet.shared });
      });

      it('given an expect that does not match, when the node is deleted, then it is text-mismatch naming the step and nothing changes, and with the right text it applies', async (ctx) => {
        if (!(await announces('deleteNode'))) return ctx.skip();
        const base = await start(snippet.branch);

        expect(failed(await client.deleteNode(second(), { expect: 'something else' }))).toMatchObject({ code: 'text-mismatch', step: 0, expected: 'something else', actual: snippet.second });
        expect(await snapshot()).toMatchObject({ text: snippet.branch, revision: base });
        unwrap(await client.deleteNode(second(), { expect: snippet.second }));

        expect(await snapshot()).toMatchObject({ text: gone().text });
      });

      it('given a syntax error elsewhere in the document, when a healthy statement is deleted, then it applies', async (ctx) => {
        if (!(await announces('deleteNode'))) return ctx.skip();
        const base = await start(snippet.broken);

        unwrap(await client.deleteNode(rangeOf(snippet.broken, snippet.second)));

        expect(await snapshot()).toMatchObject({ text: withoutLine(snippet.broken, snippet.second).text, revision: base + 1 });
      });

      it('given a node step beside a replace step and a call at an older revision, when they are sent, then the first is invalid and the second is stale even with map, and nothing changes', async (ctx) => {
        if (!(await announces('deleteNode'))) return ctx.skip();
        const base = await start(snippet.branch);

        expect(failed(await client.transact([{ op: 'deleteNode', node: second() }, { op: 'replace', range: { from: 0, to: 0 }, text: 'x' }]))).toMatchObject({ code: 'invalid', path: 'steps' });
        unwrap(await client.insertAt(0, '\n'));
        expect(failed(await client.deleteNode(second(), { at: base, stale: 'map' }))).toMatchObject({ code: 'stale', head: base + 1 });

        expect(await snapshot()).toMatchObject({ text: `\n${snippet.branch}`, revision: base + 1 });
      });

      it('given a preview, when a node is deleted, then the would-be text and its diff come back, changes is empty, and no revision, event or undo step is made', async (ctx) => {
        if (!(await announces('deleteNode'))) return ctx.skip();
        const base = await start(snippet.branch);
        const seen = changes();

        const reply = await client.deleteNode(second(), { preview: true });

        const value = unwrap(reply);
        expect(value.changes).toEqual([]);
        expect(value.preview).toMatchObject({ text: gone().text, landed: [{ from: gone().at, to: gone().at }] });
        expect(value.preview?.diff).toHaveLength(1);
        expect(reply).toMatchObject({ revision: base, base });
        expect(await snapshot()).toMatchObject({ text: snippet.branch, revision: base });
        expect(seen).toEqual([]);
        expect(unwrap(await client.undo()).applied).toBe(false);
      });

      it('given the caret inside a statement, when the selection is deleted, then the statement at the caret goes and the caret stays where its line was', async (ctx) => {
        if (!(await announces('deleteNode'))) return ctx.skip();
        await start(snippet.branch);
        unwrap(await client.select(second().from + 2));

        unwrap(await client.deleteNode('selection'));

        const after = await snapshot();
        expect(after.text).toBe(gone().text);
        expect(after.selection.ranges[0]).toEqual({ anchor: gone().at, head: gone().at });
      });

      it('given a select step that points at the deleted text, when it is sent beside the delete, then the caret lands where the text was', async (ctx) => {
        if (!(await announces('deleteNode'))) return ctx.skip();
        await start(snippet.branch);

        const reply = await client.transact([{ op: 'deleteNode', node: second() }, { op: 'select', anchor: { step: 0 } }]);

        unwrap(reply);
        expect((await snapshot()).selection.ranges[0]).toEqual({ anchor: gone().at, head: gone().at });
      });
    });

    describe('moveNode', () => {
      const second = () => rangeOf(snippet.branch, snippet.second);
      const ifNode = () => rangeOf(snippet.branch, snippet.ifNode);

      it('given a statement inside an if, when it moves after the if, then it is a sibling after the if at the if indent in one revision with one api change, landed names its lines, and one undo restores the text', async (ctx) => {
        if (!(await announces('moveNode'))) return ctx.skip();
        const base = await start(snippet.branch);
        const seen = changes();

        const reply = await client.moveNode(second(), { after: ifNode() });

        const value = unwrap(reply);
        expect(reply).toMatchObject({ revision: base + 1, base });
        expect(value.landed).toEqual([wholeLines(snippet.secondAfterIf, snippet.second)]);
        expect(await snapshot()).toMatchObject({ text: snippet.secondAfterIf, revision: base + 1 });
        expect(seen).toMatchObject([{ type: 'changed', from: base, to: base + 1, origin: 'api' }]);
        unwrap(await client.undo());
        expect(await snapshot()).toMatchObject({ text: snippet.branch });
      });

      it('given the statement after an if, when it moves to the end of the if block, then it takes the block indent', async (ctx) => {
        if (!(await announces('moveNode'))) return ctx.skip();
        await start(snippet.branch);

        unwrap(await client.moveNode(rangeOf(snippet.branch, snippet.last), { into: ifNode(), at: 'end' }));

        expect(await snapshot()).toMatchObject({ text: snippet.lastIntoIf });
        unwrap(await client.undo());
        expect(await snapshot()).toMatchObject({ text: snippet.branch });
      });

      it('given a statement with a comment line above it, when it moves to the start of a block, then the comment goes with it', async (ctx) => {
        if (!(await announces('moveNode'))) return ctx.skip();
        await start(snippet.noted);

        unwrap(await client.moveNode(rangeOf(snippet.noted, snippet.notedStatement), { into: rangeOf(snippet.noted, snippet.notedIf), at: 'start' }));

        expect(await snapshot()).toMatchObject({ text: snippet.notedMoved });
      });

      it('given a statement moved before its own next sibling, when it is applied, then it is ok with no change, no revision and no event', async (ctx) => {
        if (!(await announces('moveNode'))) return ctx.skip();
        const base = await start(snippet.branch);
        const seen = changes();

        const reply = await client.moveNode(rangeOf(snippet.branch, snippet.first), { before: second() });

        expect(unwrap(reply).changes).toEqual([]);
        expect(reply).toMatchObject({ revision: base, base });
        expect(await snapshot()).toMatchObject({ text: snippet.branch, revision: base });
        expect(seen).toEqual([]);
      });

      it('given a destination inside the moved node, when it is applied, then it is invalid naming steps[0].to and nothing changes', async (ctx) => {
        if (!(await announces('moveNode'))) return ctx.skip();
        const base = await start(snippet.branch);

        expect(failed(await client.moveNode(ifNode(), { into: ifNode(), at: 'start' }))).toMatchObject({ code: 'invalid', path: 'steps[0].to' });
        expect(failed(await client.moveNode(ifNode(), { before: rangeOf(snippet.branch, snippet.first) }))).toMatchObject({ code: 'invalid', path: 'steps[0].to' });

        expect(await snapshot()).toMatchObject({ text: snippet.branch, revision: base });
      });

      it('given the only statement of a loop, when it moves out, then Python refuses the emptied block as would-not-parse and braces leave an empty block', async (ctx) => {
        if (!(await announces('moveNode'))) return ctx.skip();
        const base = await start(snippet.loop);

        const reply = await client.moveNode(rangeOf(snippet.loop, snippet.inner), { after: rangeOf(snippet.loop, snippet.ret) });

        if (language === 'python') {
          expect(failed(reply)).toMatchObject({ code: 'would-not-parse', reason: 'syntax' });
          expect(await snapshot()).toMatchObject({ text: snippet.loop, revision: base });
          return;
        }
        unwrap(reply);
        expect(await snapshot()).toMatchObject({ text: snippet.loopEmptied, revision: base + 1 });
      });

      it('given a destination whose reference shares its line with another statement, when the node moves, then it is not-a-node naming the destination and nothing changes', async (ctx) => {
        if (!(await announces('moveNode'))) return ctx.skip();
        const base = await start(snippet.shared);
        const reference = rangeOf(snippet.shared, snippet.second);

        const reply = await client.moveNode(rangeOf(snippet.shared, snippet.last), { before: reference });

        expect(failed(reply)).toMatchObject({ code: 'not-a-node', range: reference, step: 0 });
        expect(await snapshot()).toMatchObject({ text: snippet.shared, revision: base });
      });

      it('given an expect that does not match and a call at an older revision, when the node moves, then they are text-mismatch and stale, and nothing changes', async (ctx) => {
        if (!(await announces('moveNode'))) return ctx.skip();
        const base = await start(snippet.branch);

        expect(failed(await client.moveNode(second(), { after: ifNode() }, { expect: 'something else' }))).toMatchObject({ code: 'text-mismatch', step: 0, expected: 'something else', actual: snippet.second });
        unwrap(await client.insertAt(0, '\n'));
        expect(failed(await client.moveNode(second(), { after: ifNode() }, { at: base, stale: 'map' }))).toMatchObject({ code: 'stale', head: base + 1 });

        expect(await snapshot()).toMatchObject({ text: `\n${snippet.branch}`, revision: base + 1 });
      });

      it('given the caret inside the moved statement, when it moves, then the caret is on the same character in the new place', async (ctx) => {
        if (!(await announces('moveNode'))) return ctx.skip();
        await start(snippet.branch);
        unwrap(await client.select(second().from + 2));

        unwrap(await client.moveNode(second(), { after: ifNode() }));

        const caret = snippet.secondAfterIf.indexOf(snippet.second) + 2;
        expect((await snapshot()).selection.ranges[0]).toEqual({ anchor: caret, head: caret });
      });

      it('given a document with no final newline, when its last statement moves up and when the first moves to the end, then the final newline is where the lines put it', async (ctx) => {
        if (!(await announces('moveNode'))) return ctx.skip();
        await start(snippet.flat);

        unwrap(await client.moveNode(rangeOf(snippet.flat, snippet.flatLast), { before: rangeOf(snippet.flat, snippet.flatFirst) }));
        expect(await snapshot()).toMatchObject({ text: `${snippet.flatLast}\n${snippet.flatFirst}\n` });
        await start(snippet.flat);
        unwrap(await client.moveNode(rangeOf(snippet.flat, snippet.flatFirst), { after: rangeOf(snippet.flat, snippet.flatLast) }));

        expect(await snapshot()).toMatchObject({ text: `${snippet.flatLast}\n${snippet.flatFirst}` });
      });
    });

    describe('duplicateNode', () => {
      it('given a compound statement, when it is duplicated, then the whole block is copied after it and landed names the copy, and one undo removes it', async (ctx) => {
        if (!(await announces('duplicateNode'))) return ctx.skip();
        const base = await start(snippet.branch);
        const seen = changes();
        const lines = wholeLines(snippet.branch, snippet.ifNode);
        const copy = snippet.branch.slice(lines.from, lines.to);

        const reply = await client.duplicateNode(rangeOf(snippet.branch, snippet.ifNode));

        expect(unwrap(reply).landed).toEqual([{ from: lines.to, to: lines.to + copy.length }]);
        expect(await snapshot()).toMatchObject({ text: snippet.branch.slice(0, lines.to) + copy + snippet.branch.slice(lines.to), revision: base + 1 });
        expect(seen).toMatchObject([{ type: 'changed', origin: 'api' }]);
        unwrap(await client.undo());
        expect(await snapshot()).toMatchObject({ text: snippet.branch });
      });

      it('given a one-line statement, when it is duplicated, then the copy follows with the same indent and the caret stays', async (ctx) => {
        if (!(await announces('duplicateNode'))) return ctx.skip();
        await start(snippet.branch);
        const lines = wholeLines(snippet.branch, snippet.second);
        unwrap(await client.select(lines.from + 3));

        unwrap(await client.duplicateNode(rangeOf(snippet.branch, snippet.second)));

        const after = await snapshot();
        expect(after.text).toBe(snippet.branch.slice(0, lines.to) + snippet.branch.slice(lines.from, lines.to) + snippet.branch.slice(lines.to));
        expect(after.selection.ranges[0]).toEqual({ anchor: lines.from + 3, head: lines.from + 3 });
      });

      it('given a document with no final newline, when its last statement is duplicated, then the copy comes after a newline and the text still has no final newline', async (ctx) => {
        if (!(await announces('duplicateNode'))) return ctx.skip();
        await start(snippet.flat);

        const reply = await client.duplicateNode(rangeOf(snippet.flat, snippet.flatLast));

        expect(await snapshot()).toMatchObject({ text: `${snippet.flat}\n${snippet.flatLast}` });
        expect(unwrap(reply).landed).toEqual([{ from: snippet.flat.length + 1, to: snippet.flat.length + 1 + snippet.flatLast.length }]);
      });
    });
    describe('wrapNode and unwrapNode', () => {
      const wrap = WRAPS[language];
      const pair = (): Range => ({ from: snippet.branch.indexOf(snippet.first), to: snippet.branch.indexOf(snippet.second) + snippet.second.length });
      // the new loop's own lines in the wrapped branch: its header line to its last line (the closing brace in braces)
      const loopLines = (): Range => {
        const last = wholeLines(wrap.wrapped, snippet.second);
        return { from: wholeLines(wrap.wrapped, wrap.header.slice(0, 5)).from, to: language === 'python' ? last.to : wrap.wrapped.indexOf('\n', last.to) + 1 };
      };

      it('given two statements of an if, when they are wrapped in a loop, then they sit one unit deeper under the header in one revision with one api change, landed names the loop, and one undo restores the text', async (ctx) => {
        if (!(await announces('wrapNode'))) return ctx.skip();
        const base = await start(snippet.branch);
        const seen = changes();

        const reply = await client.wrapNode(pair(), wrap.header);

        const value = unwrap(reply);
        expect(reply).toMatchObject({ revision: base + 1, base });
        expect(await snapshot()).toMatchObject({ text: wrap.wrapped, revision: base + 1 });
        expect(value.landed).toEqual([loopLines()]);
        expect(seen).toMatchObject([{ type: 'changed', from: base, to: base + 1, origin: 'api' }]);
        unwrap(await client.undo());
        expect(await snapshot()).toMatchObject({ text: snippet.branch });
      });

      // the fake editor's own Python parser reads no header: for Python these two are proved on the installed grammar (ops.wrap.test.ts)
      it('given a header that does not parse, when a statement is wrapped, then it is would-not-parse syntax with the error in the would-be text and nothing changes', async (ctx) => {
        if (!(await announces('wrapNode')) || language === 'python') return ctx.skip();
        const base = await start(snippet.branch);

        const error = failed(await client.wrapNode(pair(), wrap.broken));

        expect(error).toMatchObject({ code: 'would-not-parse', reason: 'syntax' });
        if (error.code === 'would-not-parse') expect(error.errors.length).toBeGreaterThan(0);
        expect(await snapshot()).toMatchObject({ text: snippet.branch, revision: base });
      });

      it('given a header that carries its own statement, when a statement is wrapped, then it is would-not-parse structure because the lines landed in a bare block, and nothing changes', async (ctx) => {
        if (!(await announces('wrapNode')) || language === 'python') return ctx.skip();
        const base = await start(snippet.branch);

        const error = failed(await client.wrapNode(pair(), wrap.trap));

        expect(error).toMatchObject({ code: 'would-not-parse', reason: 'structure' });
        expect(await snapshot()).toMatchObject({ text: snippet.branch, revision: base });
      });

      it('given a statement over two lines, when it is wrapped, then the second line keeps its offset to the first', async (ctx) => {
        if (!(await announces('wrapNode'))) return ctx.skip();
        await start(wrap.hang);

        unwrap(await client.wrapNode(rangeOf(wrap.hang, wrap.hangNode), wrap.header));

        expect(await snapshot()).toMatchObject({ text: wrap.hangWrapped });
      });

      it('given the caret inside a wrapped statement, when it is wrapped, then the caret is on the same character in the new place', async (ctx) => {
        if (!(await announces('wrapNode'))) return ctx.skip();
        await start(snippet.branch);
        unwrap(await client.select(snippet.branch.indexOf(snippet.second) + 2));

        unwrap(await client.wrapNode(pair(), wrap.header));

        const caret = wrap.wrapped.indexOf(snippet.second) + 2;
        expect((await snapshot()).selection.ranges[0]).toEqual({ anchor: caret, head: caret });
      });

      it('given the last statement of a document with no final newline, when it is wrapped, then the closing line ends the document and there is still no final newline', async (ctx) => {
        if (!(await announces('wrapNode'))) return ctx.skip();
        await start(wrap.tail);

        unwrap(await client.wrapNode(rangeOf(wrap.tail, wrap.tailNode), wrap.header));

        expect(await snapshot()).toMatchObject({ text: wrap.tailWrapped });
      });

      it('given a loop, when it is unwrapped, then its body replaces it one unit shallower, the header and the closing brace are gone, and one undo restores the text', async (ctx) => {
        if (!(await announces('unwrapNode'))) return ctx.skip();
        const base = await start(snippet.loop);
        const seen = changes();

        const reply = await client.unwrapNode(rangeOf(snippet.loop, wrap.loopNode));

        const value = unwrap(reply);
        expect(reply).toMatchObject({ revision: base + 1, base });
        expect(await snapshot()).toMatchObject({ text: wrap.unwrapped, revision: base + 1 });
        expect(value.landed).toEqual([wholeLines(wrap.unwrapped, snippet.inner)]);
        expect(seen).toMatchObject([{ type: 'changed', origin: 'api' }]);
        unwrap(await client.undo());
        expect(await snapshot()).toMatchObject({ text: snippet.loop });
      });

      it('given an if with an else, when it is unwrapped, then it is not-a-node not-unwrappable naming the if and nothing changes', async (ctx) => {
        if (!(await announces('unwrapNode'))) return ctx.skip();
        const base = await start(wrap.ifElse);

        const error = failed(await client.unwrapNode(rangeOf(wrap.ifElse, wrap.ifElseNode)));

        expect(error).toMatchObject({ code: 'not-a-node', reason: 'not-unwrappable', range: rangeOf(wrap.ifElse, wrap.ifElseNode), step: 0 });
        expect(await snapshot()).toMatchObject({ text: wrap.ifElse, revision: base });
      });

      it('given a plain statement, when it is unwrapped, then it is not-a-node not-unwrappable', async (ctx) => {
        if (!(await announces('unwrapNode'))) return ctx.skip();
        await start(snippet.branch);

        expect(failed(await client.unwrapNode(rangeOf(snippet.branch, snippet.second)))).toMatchObject({ code: 'not-a-node', reason: 'not-unwrappable' });
      });

      it('given a statement, when it is wrapped and then the new loop is unwrapped, then the text is the original exactly, for a block, a compound, a loop body, a commented statement and a last line with no newline', async (ctx) => {
        if (!(await announces('wrapNode')) || !(await announces('unwrapNode'))) return ctx.skip();
        const corpus: readonly (readonly [string, Range])[] = [
          [snippet.branch, pair()],
          [snippet.branch, rangeOf(snippet.branch, snippet.ifNode)],
          [snippet.loop, rangeOf(snippet.loop, snippet.inner)],
          [snippet.noted, rangeOf(snippet.noted, snippet.notedStatement)],
          [wrap.tail, rangeOf(wrap.tail, wrap.tailNode)],
        ];

        for (const [text, node] of corpus) {
          await start(text);
          const wrapped = unwrap(await client.wrapNode(node, wrap.header));
          const loop = wrapped.landed[0] as Range;

          unwrap(await client.unwrapNode(loop));

          expect((await snapshot()).text).toBe(text);
        }
      });
    });
    describe('insertSnippet', () => {
      const edit = EDITS[language];
      const after = (): Range => rangeOf(edit.even, snippet.second);

      it('given a snippet with a relative indent, when it is inserted after a statement of a nested block, then every line takes the destination indent on top of its own, landed names the snippet lines, and one undo restores the text', async (ctx) => {
        if (!(await announces('insertSnippet'))) return ctx.skip();
        const base = await start(edit.even);
        const seen = changes();

        const reply = await client.insertSnippet({ after: after() }, edit.snippet);

        const value = unwrap(reply);
        const lines = wholeLines(edit.inserted, edit.snippetFirst);
        expect(reply).toMatchObject({ revision: base + 1, base });
        expect(await snapshot()).toMatchObject({ text: edit.inserted, revision: base + 1 });
        expect(value.landed).toEqual([{ from: lines.from, to: lines.from + (edit.inserted.length - edit.even.length) }]);
        expect(seen).toMatchObject([{ type: 'changed', from: base, to: base + 1, origin: 'api' }]);
        unwrap(await client.undo());
        expect(await snapshot()).toMatchObject({ text: edit.even });
      });

      it('given a clause as the snippet, when it is inserted after the if it would extend, then it is would-not-parse structure and nothing changes', async (ctx) => {
        if (!(await announces('insertSnippet'))) return ctx.skip();
        const base = await start(edit.even);

        const error = failed(await client.insertSnippet({ after: rangeOf(edit.even, language === 'python' ? 'if x:\n        a = 1\n        b = 2' : 'if (x) {\n        a = 1;\n        b = 2;\n    }') }, edit.clause));

        expect(error).toMatchObject({ code: 'would-not-parse', reason: 'structure' });
        expect(await snapshot()).toMatchObject({ text: edit.even, revision: base });
      });

      it('given blank text, when it is inserted, then it is invalid and nothing changes', async (ctx) => {
        if (!(await announces('insertSnippet'))) return ctx.skip();
        const base = await start(edit.even);

        expect(failed(await client.insertSnippet({ after: after() }, '  \n '))).toMatchObject({ code: 'invalid' });

        expect(await snapshot()).toMatchObject({ text: edit.even, revision: base });
      });

      it('given a select step on the end edge of a snippet, when they are sent together, then the caret is at the end of the snippet', async (ctx) => {
        if (!(await announces('insertSnippet'))) return ctx.skip();
        await start(edit.even);

        const reply = await client.transact([{ op: 'insertSnippet', at: { after: after() }, text: edit.snippet }, { op: 'select', anchor: { step: 0, edge: 'end' } }]);

        const landed = unwrap(reply).landed[0] as Range;
        expect(landed.to - landed.from).toBe(edit.inserted.length - edit.even.length);
        expect((await snapshot()).selection.ranges[0]).toEqual({ anchor: landed.to, head: landed.to });
      });

      it('given a preview, when a snippet is inserted, then the would-be text comes back and nothing is applied', async (ctx) => {
        if (!(await announces('insertSnippet'))) return ctx.skip();
        const base = await start(edit.even);

        const reply = await client.insertSnippet({ after: after() }, edit.snippet, { preview: true });

        expect(unwrap(reply).preview).toMatchObject({ text: edit.inserted });
        expect(await snapshot()).toMatchObject({ text: edit.even, revision: base });
      });
    });

    describe('reindentBlock', () => {
      const edit = EDITS[language];

      it('given an if pasted with a shallower body, when it is re-indented with no count, then every line is at the depth it asks for, the brace takes the depth of its owner, the shape holds, and one undo restores the text', async (ctx) => {
        if (!(await announces('reindentBlock'))) return ctx.skip();
        const base = await start(edit.pasted);
        const seen = changes();

        const reply = await client.reindentBlock(rangeOf(edit.pasted, edit.pastedIf));

        const value = unwrap(reply);
        expect(reply).toMatchObject({ revision: base + 1, base });
        expect(await snapshot()).toMatchObject({ text: edit.even, revision: base + 1 });
        expect(value.landed).toEqual([{ from: edit.even.indexOf('    if'), to: edit.even.indexOf('    return') }]);
        expect(seen).toMatchObject([{ type: 'changed', origin: 'api' }]);
        unwrap(await client.undo());
        expect(await snapshot()).toMatchObject({ text: edit.pasted });
      });

      it('given a block that is already at its depth, when it is re-indented with no count, then it is ok with no change, no revision and no event', async (ctx) => {
        if (!(await announces('reindentBlock'))) return ctx.skip();
        const base = await start(edit.even);
        const seen = changes();

        const reply = await client.reindentBlock(rangeOf(edit.even, snippet.second));

        expect(unwrap(reply).changes).toEqual([]);
        expect(reply).toMatchObject({ revision: base, base });
        expect(await snapshot()).toMatchObject({ text: edit.even, revision: base });
        expect(seen).toEqual([]);
      });

      it('given the second statement of an if, when it is outdented by one unit, then it moves left and the text parses: Python leaves the if body, braces keep it in the block', async (ctx) => {
        if (!(await announces('reindentBlock'))) return ctx.skip();
        const base = await start(edit.even);

        unwrap(await client.reindentBlock(rangeOf(edit.even, snippet.second), -1));

        expect(await snapshot()).toMatchObject({ text: edit.outdented, revision: base + 1 });
      });

      it('given a function body with a statement over two lines, when it is re-indented with no count, then the second line keeps its offset to the first', async (ctx) => {
        if (!(await announces('reindentBlock')) || language === 'python') return ctx.skip();
        await start(edit.hang);

        unwrap(await client.reindentBlock({ from: edit.hang.indexOf('total'), to: edit.hang.lastIndexOf(';') + 1 }));

        expect(await snapshot()).toMatchObject({ text: edit.hangFixed });
      });

      it('given the caret inside a statement, when its block is re-indented, then the caret is on the same character in the new place', async (ctx) => {
        if (!(await announces('reindentBlock'))) return ctx.skip();
        await start(edit.pasted);
        unwrap(await client.select(edit.pasted.indexOf(snippet.second) + 2));

        unwrap(await client.reindentBlock(rangeOf(edit.pasted, edit.pastedIf)));

        const caret = edit.even.indexOf(snippet.second) + 2;
        expect((await snapshot()).selection.ranges[0]).toEqual({ anchor: caret, head: caret });
      });
    });
  });
}
