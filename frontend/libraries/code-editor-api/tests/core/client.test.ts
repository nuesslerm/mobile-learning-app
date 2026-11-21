import { describe, expect, it } from 'vitest';

import { createDeferredEngine, createEditorClient, createEditorEngine, unwrap, EditorCallError, type CallTrace, type EditorEngine } from '../../src';
import { createFakeEditor } from '../../src/testing';

describe('the editor client', () => {
  it('given an engine whose transport rejects, when a named call runs, then it resolves a transport reply and does not reject', async () => {
    // Given an engine that fails like a closed channel
    const failing: EditorEngine = { request: () => Promise.reject(Object.assign(new Error('gone'), { code: 'closed' })), listen: () => () => undefined };

    // When a named call runs
    const reply = await createEditorClient(failing).getText();

    // Then the failure is a value
    expect(reply).toEqual({ ok: false, revision: null, error: { code: 'transport', message: 'gone', cause: 'closed' } });
  });

  it('given a trace hook, when calls succeed and fail, then each is traced with its outcome', async () => {
    const traces: CallTrace[] = [];
    const client = createEditorClient(createFakeEditor({ text: 'abc' }).engine, { trace: (call) => traces.push(call), now: () => 1 });
    await client.getText();
    await client.getText({ from: 0, to: 99 });
    expect(traces).toMatchObject([{ name: 'read', ok: true, revision: 0 }, { name: 'read', ok: false, code: 'out-of-range' }]);
  });

  it('given a failed reply, when it is unwrapped, then it throws an EditorCallError carrying the error', async () => {
    const reply = await createEditorClient(createFakeEditor().engine).getText({ from: 0, to: 9 });
    expect(() => unwrap(reply)).toThrow(EditorCallError);
  });
});

const reportedByDeferred: unknown[] = [];

describe('the deferred engine', () => {
  it('given no editor yet, when it is asked, then hello is not ready and the rest is not-ready; binding emits ready with the new instance', async () => {
    // Given a deferred engine and a ready listener
    const deferred = createDeferredEngine((error) => reportedByDeferred.push(error));
    const seen: unknown[] = [];
    deferred.listen(['ready'], (event) => seen.push(event));
    expect(await deferred.request('hello', undefined)).toMatchObject({ ok: true, value: { ready: false } });
    expect(await deferred.request('read', {})).toMatchObject({ ok: false, error: { code: 'not-ready' } });

    // When an engine is bound, and then another
    const first = createFakeEditor({ instance: 'one' });
    deferred.bind(first.engine);
    deferred.bind(createFakeEditor({ instance: 'two' }).engine);

    // Then each bind announced its instance and requests reach the engine
    expect(seen).toEqual([{ type: 'ready', instance: 'one', revision: 0 }, { type: 'ready', instance: 'two', revision: 0 }]);
    expect(await deferred.request('hello', undefined)).toMatchObject({ value: { ready: true, instance: 'two' } });
  });

  it('given a host that throws, when a request runs, then the reply is internal and the error is reported', async () => {
    const reported: unknown[] = [];
    const fake = createFakeEditor();
    const broken = createEditorEngine({ ...hostOf(fake), sync: () => { throw new Error('host broke'); } }, { instance: 'x', logError: (e) => reported.push(e) });
    expect(await broken.request('read', {})).toMatchObject({ ok: false, error: { code: 'internal', message: 'host broke' } });
    expect(reported).toHaveLength(1);
  });
});

function hostOf(fake: ReturnType<typeof createFakeEditor>) {
  return {
    doc: { get length() { return fake.text().length; }, slice: (from: number, to: number) => fake.text().slice(from, to) },
    selection: () => fake.peek().selection,
    hasFocus: () => false,
    sync: () => undefined,
    commit: () => undefined,
    history: () => false,
    load: () => ({ text: '', selection: { ranges: [{ anchor: 0, head: 0 }], main: 0 }, revision: 1 }),
    configure: () => ({ ok: false as const, error: { code: 'internal' as const, message: 'n/a' } }),
    setFocus: () => undefined,
    languages: () => ['python' as const],
    maxSelections: 1,
  };
}

describe('an editor whose host cannot type', () => {
  it('given a host without the type method, when hello and a type step are sent, then the step is not announced and the call is unsupported', async () => {
    // Given a fake editor without the optional host method
    const fake = createFakeEditor({ text: 'abc', typing: false, completion: false });
    const client = createEditorClient(fake.engine);

    // When hello and a type step are sent
    const hello = unwrap(await client.hello());
    const reply = await client.transact([{ op: 'type', text: 'x' }]);

    // Then the step is not announced, the call is unsupported, and the text is unchanged
    expect(hello.capabilities.steps).toEqual(['replace', 'move', 'select']);
    expect(reply).toMatchObject({ ok: false, error: { code: 'unsupported', what: 'step:type' } });
    expect(fake.text()).toBe('abc');
  });
});

describe('an editor whose host cannot complete', () => {
  it('given a host without the complete method, when hello, a complete step and a completion config are sent, then none is announced and each is unsupported', async () => {
    const fake = createFakeEditor({ text: 'abc', completion: false });
    const client = createEditorClient(fake.engine);

    const hello = unwrap(await client.hello());
    const step = await client.transact([{ op: 'complete' }]);
    const config = await client.setConfig({ completion: { enabled: true, entries: [] } });

    expect(hello.capabilities.steps).toEqual(['replace', 'move', 'select', 'type']);
    expect(hello.capabilities.events).not.toContain('completion');
    expect(step).toMatchObject({ ok: false, error: { code: 'unsupported', what: 'step:complete' } });
    expect(config).toMatchObject({ ok: false, error: { code: 'unsupported', what: 'config:completion' } });
    expect(fake.text()).toBe('abc');
  });
});

describe('the completion event', () => {
  it('given the engine told twice that a list is open and then closed, when a listener hears completion, then it hears each change once', () => {
    // Given a listener on completion
    const fake = createFakeEditor({ text: 'abc' });
    const heard: unknown[] = [];
    fake.engine.listen(['completion'], (event) => heard.push(event));

    // When the host reports open, open again, then closed
    fake.engine.ingestCompletion(true);
    fake.engine.ingestCompletion(true);
    fake.engine.ingestCompletion(false);

    // Then one event per change, in order, at the head
    expect(heard).toEqual([
      { type: 'completion', open: true, revision: 0 },
      { type: 'completion', open: false, revision: 0 },
    ]);
  });
});

describe('the editor client over the tree request', () => {
  const source = 'def f(xs):\n    for x in xs:\n        print(x)\n    return 1\n';
  const spyOn = (engine: EditorEngine, after?: (name: string, reads: number) => void) => {
    const calls: { name: string; params: unknown }[] = [];
    let reads = 0;
    const spy: EditorEngine = {
      listen: (types, onEvent) => engine.listen(types, onEvent),
      request: async (name, params) => {
        calls.push({ name, params });
        const reply = await engine.request(name, params);
        if (name === 'read') reads += 1;
        after?.(name, reads);
        return reply;
      },
    };
    return { spy, calls };
  };

  it('given an engine whose transport rejects, when tree runs, then it resolves a transport reply and does not reject', async () => {
    const failing: EditorEngine = { request: () => Promise.reject(Object.assign(new Error('gone'), { code: 'closed' })), listen: () => () => undefined };

    expect(await createEditorClient(failing).tree()).toMatchObject({ ok: false, error: { code: 'transport', cause: 'closed' } });
  });

  it('given a spy engine, when nodeAt runs, then exactly one tree request goes out with a point range and full detail', async () => {
    const { spy, calls } = spyOn(createFakeEditor({ text: source, tree: true }).engine);

    const reply = await createEditorClient(spy).nodeAt(source.indexOf('print') + 1);

    expect(calls).toEqual([{ name: 'tree', params: { range: { from: source.indexOf('print') + 1, to: source.indexOf('print') + 1 }, detail: 'full' } }]);
    expect(unwrap(reply)).toMatchObject({ type: expect.any(String) });
    expect(reply).toMatchObject({ ok: true, revision: 0, base: 0 });
  });

  it('given a caret in a loop body, when statementAt and enclosingBlock run, then the statement is the call and the block is owned by the loop', async () => {
    const client = createEditorClient(createFakeEditor({ text: source, tree: true }).engine);
    const caret = source.indexOf('print') + 2;

    const statement = unwrap(await client.statementAt(caret));
    const block = unwrap(await client.enclosingBlock(caret));

    expect(source.slice(statement?.from, statement?.to)).toBe('print(x)');
    expect(source.startsWith('for', block?.owner?.from)).toBe(true);
  });

  it('given a range, when nodesInRange runs, then the nodes inside it come back with within', async () => {
    const client = createEditorClient(createFakeEditor({ text: source, tree: true }).engine);
    const from = source.indexOf('return');

    const found = unwrap(await client.nodesInRange({ from, to: from + 8 }, { within: true }));

    expect(found.map((node) => node.type)).toEqual(['ReturnStatement']);
  });

  it('given a keystroke between the text read and the tree, when enclosingBlock runs, then it reads again once and answers for the newer revision', async () => {
    const fake = createFakeEditor({ text: source, tree: true });
    let typed = false;
    const { spy, calls } = spyOn(fake.engine, (name) => {
      if (name === 'read' && !typed) {
        typed = true;
        fake.userTypes('\n');
      }
    });

    const reply = await createEditorClient(spy).enclosingBlock(source.indexOf('print') + 3);

    expect(calls.map((call) => call.name)).toEqual(['read', 'tree', 'read', 'tree']);
    expect(reply).toMatchObject({ ok: true, base: 1, revision: 1 });
  });

  it('given a keystroke after every read, when enclosingBlock runs, then it gives up after one retry with stale', async () => {
    const fake = createFakeEditor({ text: source, tree: true });
    const { spy, calls } = spyOn(fake.engine, (name) => {
      if (name === 'read') fake.userTypes('\n');
    });

    const reply = await createEditorClient(spy).enclosingBlock(source.indexOf('print') + 3);

    expect(reply).toMatchObject({ ok: false, error: { code: 'stale' } });
    expect(calls.filter((call) => call.name === 'read')).toHaveLength(2);
  });

  it('given a mirror text, when enclosingBlock runs, then the text is not read again', async () => {
    const { spy, calls } = spyOn(createFakeEditor({ text: source, tree: true }).engine);

    const reply = await createEditorClient(spy).enclosingBlock(source.indexOf('print') + 3, { text: source });

    expect(calls.map((call) => call.name)).toEqual(['tree']);
    expect(unwrap(reply)?.owner).not.toBeNull();
  });

  it('given a blank indented line below a body, when enclosingBlock runs, then the tree asked for starts at the line above so the body is found', async () => {
    const text = 'def f(xs):\n    return 1\n    ';
    const client = createEditorClient(createFakeEditor({ text, tree: true }).engine);

    const block = unwrap(await client.enclosingBlock(text.length));

    expect(block?.block.type).toBe('Body');
    expect(block?.owner?.type).toBe('FunctionDefinition');
  });

  it('given an engine without tree, when enclosingBlock runs, then the reply is unsupported', async () => {
    const client = createEditorClient(createFakeEditor({ text: source }).engine);

    expect(await client.enclosingBlock(3)).toMatchObject({ ok: false, error: { code: 'unsupported', what: 'request:tree' } });
  });

  it('given a tree read only part way, when nodeAt asks past what was parsed, then the answer is null', async () => {
    let time = 0;
    const fake = createFakeEditor({ text: source, tree: true, engine: { now: () => (time += 30) } });
    fake.slowParse?.(100_000);
    const client = createEditorClient(fake.engine);

    expect(unwrap(await client.nodeAt(source.length - 2))).toBeNull();
    expect(unwrap(await client.tree()).complete).toBe(false);
  });
});
