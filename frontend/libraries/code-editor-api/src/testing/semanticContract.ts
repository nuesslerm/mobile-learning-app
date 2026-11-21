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
  },
  javascript: {
    branch: 'function f(x) {\n  if (x) {\n    a = 1;\n    b = 2;\n  }\n  return b;\n}\n',
    second: 'b = 2;',
    loop: 'function f(xs) {\n  for (const x of xs) {\n    g(x);\n  }\n  return 1;\n}\n',
    inner: 'g(x);',
    bare: 'function f(x) {\n  return x;\n}',
    shared: 'function f(x) {\n  a = 1; b = 2;\n  return b;\n}\n',
    broken: 'function g() {\n  let a = ;\n}\n\nfunction f(x) {\n  if (x) {\n    a = 1;\n    b = 2;\n  }\n  return b;\n}\n',
  },
  cpp: {
    branch: 'int f(int x) {\n  if (x) {\n    a = 1;\n    b = 2;\n  }\n  return b;\n}\n',
    second: 'b = 2;',
    loop: 'int f() {\n  for (int i = 0; i < 3; i++) {\n    g(i);\n  }\n  return 1;\n}\n',
    inner: 'g(i);',
    bare: 'int f() {\n  return 1;\n}',
    shared: 'int f(int x) {\n  a = 1; b = 2;\n  return b;\n}\n',
    broken: 'int g() {\n  int a = ;\n}\n\nint f(int x) {\n  if (x) {\n    a = 1;\n    b = 2;\n  }\n  return b;\n}\n',
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
      unwrap(await client.setConfig({ language }));
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
  });
}
