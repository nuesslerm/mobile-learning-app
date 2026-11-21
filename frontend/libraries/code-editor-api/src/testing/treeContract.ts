import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, type TestContext } from 'vitest';

import { createEditorClient, unwrap, type EditorClient } from '../core/client';
import { classify, enclosingBlock, errorRanges, nodesInRange } from '../core/syntax';
import type { EditorEvent, Language, Reply, TreeParams, TreeValue } from '../ports/protocol';
import type { MakeSubject, Subject } from './contract';

type Snippet = {
  /** a function holding a loop that holds a call, and a statement after the loop */
  readonly text: string;
  readonly inLoop: string;
  readonly loopHeader: string;
  readonly loopStarts: string;
  readonly functionStarts: string;
  /** the same shape with the loop header half typed (indentation) or the function's brace never closed (braces) */
  readonly broken: { readonly text: string; readonly at: string; readonly after: string };
  readonly statement: (index: number) => string;
};

const SNIPPETS: Readonly<Record<Language, Snippet>> = {
  python: {
    text: 'def f(xs):\n    for x in xs:\n        print(x)\n    return 1\n',
    inLoop: 'print(x)',
    loopHeader: 'for x',
    loopStarts: 'for',
    functionStarts: 'def',
    broken: { text: 'def f(xs):\n    for x in\n    return 1\n', at: 'for x in', after: 'return 1' },
    statement: (index) => `x${index} = ${index}`,
  },
  javascript: {
    text: 'function f(xs) {\n  for (const x of xs) {\n    g(x);\n  }\n  return 1;\n}\n',
    inLoop: 'g(x)',
    loopHeader: 'for (',
    loopStarts: 'for',
    functionStarts: 'function',
    broken: { text: 'function f(xs) {\n  g(xs);\n  return 1;\n', at: 'return 1;', after: 'g(xs)' },
    statement: (index) => `x${index} = ${index};`,
  },
  cpp: {
    text: 'int f() {\n  for (int i = 0; i < 3; i++) {\n    g(i);\n  }\n  return 1;\n}\n',
    inLoop: 'g(i)',
    loopHeader: 'for (',
    loopStarts: 'for',
    functionStarts: 'int',
    broken: { text: 'int f() {\n  g(1);\n  return 1;\n', at: 'return 1;', after: 'g(1)' },
    statement: (index) => `int x${index} = ${index};`,
  },
};

const failed = (reply: Reply<unknown>) => {
  if (reply.ok) throw new Error('the call was expected to fail');
  return reply.error;
};

/**
 * What the `tree` request must show on every editor that announces it, for one language. A separate suite from `editorContract`:
 * a real page runs one language per init, and every case skips unless `hello` advertises the request. Cases read the tree through
 * `classify` and `enclosingBlock`, never through leaf names, which differ between a real grammar and the fake's parsers.
 */
export function syntaxTreeContract(name: string, make: MakeSubject, language: Language): void {
  const snippet = SNIPPETS[language];
  describe(`the syntax tree contract (${language}) over ${name}`, () => {
    let subject: Subject;
    let client: EditorClient;
    const cleanups: (() => void)[] = [];
    const extras: Subject[] = [];

    beforeAll(async () => {
      subject = await make({ bound: true });
      client = createEditorClient(subject.engine);
    });
    afterAll(async () => {
      await subject.dispose();
      for (const extra of extras) await extra.dispose();
    });
    beforeEach(async (ctx) => {
      const hello = unwrap(await client.hello());
      if (!hello.capabilities.requests.includes('tree') || !hello.capabilities.languages.includes(language)) return ctx.skip();
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
    const tree = (params: TreeParams = {}): Promise<Reply<TreeValue>> => subject.engine.request('tree', params);
    const read = async (params: TreeParams = {}): Promise<TreeValue> => unwrap(await tree(params));
    const head = async (): Promise<number> => (await client.getText()).revision as number;
    const blockAt = (value: TreeValue, text: string, pos: number) => enclosingBlock(value, text, pos);
    const startsWith = (text: string, from: number, word: string) => text.startsWith(word, from);

    it('given a function with a loop inside, when tree is requested, then base equals the head, the value names that revision, the root spans what was parsed and the tree is complete', async () => {
      const revision = await start(snippet.text);

      const reply = await tree();

      expect(reply).toMatchObject({ ok: true, base: revision, revision });
      const value = unwrap(reply);
      expect(value).toMatchObject({ revision, language, length: snippet.text.length, parsedTo: snippet.text.length, complete: true, detail: 'outline' });
      expect(value.root).toMatchObject({ from: 0, to: snippet.text.length });
      expect(value.instance.length).toBeGreaterThan(0);
    });

    it('given the caret in the loop body, when the enclosing block is read, then it is owned by the loop, and on the loop line it is owned by the function', async () => {
      await start(snippet.text);
      const value = await read();

      const inner = blockAt(value, snippet.text, snippet.text.indexOf(snippet.inLoop) + 2);
      const outer = blockAt(value, snippet.text, snippet.text.indexOf(snippet.loopHeader) + 3);

      expect(inner?.owner && classify(language, inner.owner)).toBe('compound');
      expect(startsWith(snippet.text, inner?.owner?.from ?? -1, snippet.loopStarts)).toBe(true);
      expect(outer?.owner && classify(language, outer.owner)).toBe('compound');
      expect(startsWith(snippet.text, outer?.owner?.from ?? -1, snippet.functionStarts)).toBe(true);
    });

    it('given broken text, then the tree carries an error where it breaks and the block around the break is found', async () => {
      await start(snippet.broken.text);
      const value = await read();
      const at = snippet.broken.text.indexOf(snippet.broken.at) + snippet.broken.at.length;

      expect(errorRanges(value.root).length).toBeGreaterThan(0);
      if (language === 'python') {
        expect(errorRanges(value.root)[0]?.from).toBeLessThanOrEqual(at);
        expect(blockAt(value, snippet.broken.text, snippet.broken.text.indexOf(snippet.broken.after) + 3)?.damaged).toBe(false);
      } else {
        expect(blockAt(value, snippet.broken.text, snippet.broken.text.length)).toMatchObject({ closed: false, damaged: true });
      }
    });

    it('given a point range in a long document, then only the nodes touching it come back, in under 2 KB; given the whole document, then the outline of 200 lines is under 20 KB', async () => {
      const lines = Array.from({ length: 200 }, (_, index) => snippet.statement(index));
      const text = `${lines.join('\n')}\n`;
      await start(text);
      const middle = text.indexOf(snippet.statement(100));

      const point = await tree({ range: { from: middle + 1, to: middle + 1 } });
      const whole = await tree();

      expect(JSON.stringify(point).length).toBeLessThan(2_000);
      expect(nodesInRange(unwrap(point).root, { from: middle + 1, to: middle + 1 }).length).toBeGreaterThan(0);
      expect(JSON.stringify(unwrap(point).root).length).toBeLessThan(JSON.stringify(unwrap(whole).root).length);
      expect(JSON.stringify(whole).length).toBeLessThan(20_000);
    });

    it('given a tree call, then no changed, selection or focus event fires and the revision is unchanged', async () => {
      const revision = await start(snippet.text);
      const seen: EditorEvent[] = [];
      cleanups.push(subject.engine.listen(['changed', 'selection', 'focus'], (event) => seen.push(event)));

      await tree();
      await tree({ detail: 'full' });

      expect(seen).toEqual([]);
      expect(await head()).toBe(revision);
    });

    it('given an edit after a tree, then the next tree has the new revision and the old value still says the old one', async () => {
      const first = await start(snippet.text);
      const before = await read();

      const edit = await client.insertAt(0, '\n');
      unwrap(edit);
      const after = await read();

      expect(before.revision).toBe(first);
      expect(after.revision).toBe(edit.revision);
      expect(after.revision).toBeGreaterThan(before.revision);
      expect(after.length).toBe(snippet.text.length + 1);
    });

    it('given a range past the end, then out-of-range; given bad params, then invalid with the path', async () => {
      await start(snippet.text);

      expect(failed(await tree({ range: { from: 0, to: snippet.text.length + 5 } })).code).toBe('out-of-range');
      expect(failed(await tree({ budgetMs: -1 }))).toMatchObject({ code: 'invalid', path: 'budgetMs' });
    });

    it('given a page with no editor yet, when tree is requested, then it is not ready', async () => {
      const unbound = await make({ bound: false });
      extras.push(unbound);

      expect(failed(await unbound.engine.request('tree', {})).code).toBe('not-ready');
    });

    it('given every tree reply over a transport, then it survives JSON unchanged', async () => {
      await start(snippet.text);

      for (const params of [{}, { detail: 'full' as const }, { range: { from: 2, to: 2 } }]) {
        const reply = await tree(params);
        expect(JSON.parse(JSON.stringify(reply))).toEqual(reply);
      }
    });

    it('given a tree waiting for a slow parse and an apply sent after it, then the apply resolves first and the tree says it is incomplete for the revision before the apply', async (ctx: TestContext) => {
      if (!subject.hooks.slowParse) return ctx.skip();
      const first = await start(snippet.text);
      subject.hooks.slowParse(100_000);
      cleanups.push(() => subject.hooks.slowParse?.(1));
      const order: string[] = [];

      const pending = tree({ budgetMs: 60 }).then((reply) => (order.push('tree'), reply));
      const applied = client.insertAt(0, '\n').then((reply) => (order.push('apply'), reply));
      await Promise.all([pending, applied]);

      expect(order[0]).toBe('apply');
      expect(unwrap(await pending)).toMatchObject({ complete: false, revision: first });
      expect((await applied).revision as number).toBeGreaterThan(first);
    });
  });
}
