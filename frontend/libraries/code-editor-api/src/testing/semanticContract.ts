import { afterAll, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { createEditorClient, unwrap, type EditorClient } from '../core/client';
import { isSemanticOp } from '../core/validate';
import { SEMANTIC_STEP_OPS, type EditorError, type Language, type Range, type Reply, type SemanticStepOp, type Step } from '../ports/protocol';
import type { MakeSubject, Subject } from './contract';

type Snippet = {
  /** a function with an if holding two statements, and a statement after the if */
  readonly branch: string;
  readonly second: string;
  /** a function with a loop holding a call, and a statement after the loop */
  readonly loop: string;
  /** a function whose last line has no newline */
  readonly bare: string;
};

const SNIPPETS: Readonly<Record<Language, Snippet>> = {
  python: {
    branch: 'def f(x):\n    if x:\n        a = 1\n        b = 2\n    return b\n',
    second: 'b = 2',
    loop: 'def f(xs):\n    for x in xs:\n        print(x)\n    return 1\n',
    bare: 'def f(x):\n    return x',
  },
  javascript: {
    branch: 'function f(x) {\n  if (x) {\n    a = 1;\n    b = 2;\n  }\n  return b;\n}\n',
    second: 'b = 2;',
    loop: 'function f(xs) {\n  for (const x of xs) {\n    g(x);\n  }\n  return 1;\n}\n',
    bare: 'function f(x) {\n  return x;\n}',
  },
  cpp: {
    branch: 'int f(int x) {\n  if (x) {\n    a = 1;\n    b = 2;\n  }\n  return b;\n}\n',
    second: 'b = 2;',
    loop: 'int f() {\n  for (int i = 0; i < 3; i++) {\n    g(i);\n  }\n  return 1;\n}\n',
    bare: 'int f() {\n  return 1;\n}',
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

    const start = async (text: string): Promise<number> => {
      const reply = await client.load(text);
      unwrap(reply);
      return reply.revision as number;
    };
    const snapshot = async () => {
      const reply = await client.getText();
      return { ...unwrap(reply), revision: reply.revision as number };
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
  });
}
