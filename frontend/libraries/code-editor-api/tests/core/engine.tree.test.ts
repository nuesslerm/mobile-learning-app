import { describe, expect, it } from 'vitest';

import { createEditorClient, unwrap } from '../../src';
import type { TreeSnapshot } from '../../src/ports/host';
import { createFakeEditor } from '../../src/testing';

// a parse that waits for the test: each turn it is given parses `step` more characters
function manual(text: string, step: number) {
  let done = 0;
  const releases: (() => void)[] = [];
  const snapshot: TreeSnapshot = {
    language: 'python',
    length: text.length,
    work(upto) {
      done = Math.min(text.length, done + step);
      return done >= upto;
    },
    parsedTo: () => done,
    read: (range, detail) => ({ type: `Script:${detail}`, from: range.from, to: Math.min(range.to, done) }),
  };
  return {
    tree: () => snapshot,
    yieldToEvents: () => new Promise<void>((resolve) => releases.push(resolve)),
    release: () => releases.splice(0).forEach((resolve) => resolve()),
    waiting: () => releases.length,
  };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('the tree request on the engine', () => {
  it('given a host with tree(), when hello is read, then the request is announced; without it the request is unsupported', async () => {
    const text = 'abcdef';
    const parse = manual(text, 100);
    const withTree = createEditorClient(createFakeEditor({ text, tree: parse.tree, engine: { yieldToEvents: parse.yieldToEvents } }).engine);
    const without = createEditorClient(createFakeEditor({ text }).engine);

    expect(unwrap(await withTree.hello()).capabilities.requests).toContain('tree');
    expect(unwrap(await without.hello()).capabilities.requests).not.toContain('tree');
    expect(await without.engine.request('tree', {})).toMatchObject({ ok: false, error: { code: 'unsupported', what: 'request:tree' } });
  });

  it('given a small document, when tree is asked, then it is answered at once, complete, stamped with the instance and the revision it describes', async () => {
    const text = 'abcdef';
    const parse = manual(text, 100);
    const fake = createFakeEditor({ text, instance: 'tree-test', tree: parse.tree, engine: { yieldToEvents: parse.yieldToEvents } });

    const reply = await fake.engine.request('tree', {});

    expect(reply).toMatchObject({ ok: true, base: 0, revision: 0, value: { instance: 'tree-test', revision: 0, language: 'python', length: 6, parsedTo: 6, complete: true, detail: 'outline', root: { type: 'Script:outline', from: 0, to: 6 } } });
  });

  it('given a parse waiting for a turn, when an apply is asked after it, then the apply is answered first and the tree says it describes the revision before', async () => {
    // Given a tree that needs several turns
    const text = 'a'.repeat(40);
    const parse = manual(text, 8);
    const fake = createFakeEditor({ text, tree: parse.tree, engine: { yieldToEvents: parse.yieldToEvents, now: () => 0 } });
    const client = createEditorClient(fake.engine);
    const order: string[] = [];
    const treePromise = fake.engine.request('tree', { budgetMs: 50 }).then((reply) => (order.push('tree'), reply));
    await settle();
    expect(parse.waiting()).toBe(1);

    // When an apply is asked while the tree waits
    const applied = client.insertAt(0, 'Z').then((reply) => (order.push('apply'), reply));
    await settle();
    parse.release();
    await settle();
    parse.release();
    await settle();
    parse.release();
    await settle();

    // Then the apply resolved first, and the tree describes the revision before it
    expect(order[0]).toBe('apply');
    expect(unwrap(await applied).changes).toHaveLength(1);
    const reply = await treePromise;
    expect(reply).toMatchObject({ ok: true, base: 0, revision: 1, value: { revision: 0, complete: false } });
  });

  it('given a load while the tree waits, when the parse resumes, then it stops incomplete and the tree still names the revision it began at', async () => {
    const text = 'a'.repeat(40);
    const parse = manual(text, 8);
    const fake = createFakeEditor({ text, tree: parse.tree, engine: { yieldToEvents: parse.yieldToEvents, now: () => 0 } });
    const client = createEditorClient(fake.engine);
    const pending = fake.engine.request('tree', { budgetMs: 50 });
    await settle();

    unwrap(await client.load('replaced'));
    parse.release();
    const reply = await pending;

    expect(reply).toMatchObject({ ok: true, value: { complete: false, revision: 0 } });
  });

  it('given a range past the document, when tree is asked, then it is out-of-range and names the length', async () => {
    const parse = manual('abc', 100);
    const fake = createFakeEditor({ text: 'abc', tree: parse.tree });

    expect(await fake.engine.request('tree', { range: { from: 0, to: 9 } })).toMatchObject({ ok: false, error: { code: 'out-of-range', length: 3 } });
  });

  it('given bad params, when tree is asked, then it is invalid and names the path', async () => {
    const parse = manual('abc', 100);
    const fake = createFakeEditor({ text: 'abc', tree: parse.tree });

    expect(await fake.engine.request('tree', { budgetMs: -1 })).toMatchObject({ ok: false, error: { code: 'invalid', path: 'budgetMs' } });
  });

  it('given a parser that throws, when tree is asked, then the reply is internal, the error is logged and the next request is still answered', async () => {
    const errors: unknown[] = [];
    const fake = createFakeEditor({
      text: 'abc',
      logError: (error) => errors.push(error),
      tree: () => ({ language: 'python', length: 3, work: () => { throw new Error('the parser broke'); }, parsedTo: () => 0, read: () => null }),
    });

    expect(await fake.engine.request('tree', {})).toMatchObject({ ok: false, error: { code: 'internal', message: 'the parser broke' } });
    expect(errors).toHaveLength(1);
    expect(await fake.engine.request('read', {})).toMatchObject({ ok: true });
  });
});
