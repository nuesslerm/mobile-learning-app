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
    const fake = createFakeEditor({ text: 'abc', typing: false });
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
