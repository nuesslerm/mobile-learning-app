import { createDeferredEngine, createEditorClient, type CallTrace, type EditorClient } from '@lib/code-editor-api';
import { createFakeEditor } from '@lib/code-editor-api/testing';
import { act, renderHook, waitFor } from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it } from 'vitest';

import { forgetLoggedLines, loggedLines } from '../../../../../tests/flow/loggedLines';
import type { BlockEditor } from '../../../domain/blocks/blockEditor';
import { useBlockModel } from '../useBlockModel';

const DEF_IF = ['def f(x):', '    if x:', '        a = 1', '        b = 2', '    return b', ''].join('\n');

function mount(text: string, options: { editor?: (client: EditorClient) => BlockEditor } = {}) {
  const calls: CallTrace[] = [];
  const fake = createFakeEditor({ text, tree: true, instance: 'page-1' });
  const client = createEditorClient(fake.engine, { trace: (call) => calls.push(call) });
  const editor = options.editor?.(client) ?? { client, settled: () => Promise.resolve() };
  return { fake, client, calls, editor };
}

const treeReads = (calls: readonly CallTrace[]) => calls.filter((call) => call.name === 'tree').length;
const readLines = async () => (await loggedLines('leetcode.blocks.read')).length;

beforeEach(forgetLoggedLines);

describe('useBlockModel', () => {
  it('given the editor holds a def with an if, when the hook mounts, then it reads once and exposes the rows', async () => {
    const { editor, calls } = mount(DEF_IF);

    const { result } = renderHook(() => useBlockModel(editor));
    expect(result.current.status).toBe('waiting');
    await waitFor(() => expect(result.current.status).toBe('ready'));

    expect(result.current.model?.rows.map((row) => row.label)).toEqual(['def f(x):', 'if x:', 'a = 1', 'b = 2', 'return b']);
    expect(treeReads(calls)).toBe(1);
  });

  it('given a burst of keystrokes, when the hook has read, then it ends on the rows of the final text and read at most twice more', async () => {
    const { editor, fake, calls } = mount(DEF_IF);
    const { result } = renderHook(() => useBlockModel(editor));
    await waitFor(() => expect(result.current.status).toBe('ready'));
    const before = treeReads(calls);

    act(() => {
      fake.userSelects(DEF_IF.length);
      for (const key of ['x', ' ', '=', ' ', '1']) fake.userTypes(key);
    });
    await waitFor(() => expect(result.current.model?.rows.at(-1)?.label).toBe('x = 1'));

    expect(result.current.model?.text).toBe(`${DEF_IF}x = 1`);
    expect(treeReads(calls) - before).toBeLessThanOrEqual(2);
  });

  it('given the model is held, when the text changes and the hold is released, then the model stays the old one until the release and then follows the text', async () => {
    const { editor, fake } = mount(DEF_IF);
    const { result } = renderHook(() => useBlockModel(editor));
    await waitFor(() => expect(result.current.status).toBe('ready'));
    await forgetLoggedLines();

    act(() => result.current.hold());
    act(() => {
      fake.userSelects(DEF_IF.length);
      fake.userTypes('z = 0');
    });
    await waitFor(async () => expect(await readLines()).toBe(1));

    expect(result.current.model?.text).toBe(DEF_IF);
    act(() => result.current.release());
    await waitFor(() => expect(result.current.model?.text).toBe(`${DEF_IF}z = 0`));
  });

  it('given the model is held and nothing changes, when the hold is released, then the model is the one it was', async () => {
    const { editor } = mount(DEF_IF);
    const { result } = renderHook(() => useBlockModel(editor));
    await waitFor(() => expect(result.current.status).toBe('ready'));
    const shown = result.current.model;

    act(() => result.current.hold());
    act(() => result.current.release());

    expect(result.current.model).toBe(shown);
  });

  it('given a page that starts again as another editor, when it says ready, then the hook reads again and shows the new page\'s rows', async () => {
    const first = createFakeEditor({ text: DEF_IF, tree: true, instance: 'page-1' });
    const second = createFakeEditor({ text: 'x = 1\n', tree: true, instance: 'page-2' });
    const deferred = createDeferredEngine(() => undefined);
    deferred.bind(first.engine);
    const editor: BlockEditor = { client: createEditorClient(deferred), settled: () => Promise.resolve() };
    const { result } = renderHook(() => useBlockModel(editor));
    await waitFor(() => expect(result.current.model?.instance).toBe('page-1'));

    act(() => deferred.bind(second.engine));
    await waitFor(() => expect(result.current.model?.instance).toBe('page-2'));

    expect(result.current.model?.rows.map((row) => row.label)).toEqual(['x = 1']);
  });

  it('given no editor yet, when the hook mounts, then it waits and calls nothing, and reads once the editor comes', async () => {
    const { editor, calls } = mount(DEF_IF);
    const { result, rerender } = renderHook(({ current }: { current: BlockEditor | null }) => useBlockModel(current), { initialProps: { current: null as BlockEditor | null } });

    expect(result.current).toMatchObject({ status: 'waiting', model: null });
    expect(calls).toHaveLength(0);
    rerender({ current: editor });
    await waitFor(() => expect(result.current.status).toBe('ready'));

    expect(treeReads(calls)).toBe(1);
  });

  it('given the editor goes away after a read, when the hook is given none, then it waits again and shows no rows', async () => {
    const { editor } = mount(DEF_IF);
    const { result, rerender } = renderHook(({ current }: { current: BlockEditor | null }) => useBlockModel(current), { initialProps: { current: editor as BlockEditor | null } });
    await waitFor(() => expect(result.current.status).toBe('ready'));

    rerender({ current: null });

    expect(result.current).toMatchObject({ status: 'waiting', model: null });
  });

  it('given the writes the editor still has queued, when the hook reads, then it waits for them before it asks the page', async () => {
    let release!: () => void;
    const queued = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { editor, calls } = mount(DEF_IF, { editor: (client) => ({ client, settled: () => queued }) });
    const { result } = renderHook(() => useBlockModel(editor));

    expect(calls).toHaveLength(0);
    release();
    await waitFor(() => expect(result.current.status).toBe('ready'));

    expect(treeReads(calls)).toBe(1);
  });

  it('given the hook unmounts while a read waits, when the read ends, then nothing is delivered and later changes are not read', async () => {
    let release!: () => void;
    const queued = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { editor, fake, calls } = mount(DEF_IF, { editor: (client) => ({ client, settled: () => queued }) });
    const { result, unmount } = renderHook(() => useBlockModel(editor));

    unmount();
    release();
    await waitFor(async () => expect(await readLines()).toBe(1));
    const reads = treeReads(calls);
    act(() => fake.userTypes('#'));

    expect(result.current.status).toBe('waiting');
    expect(treeReads(calls)).toBe(reads);
  });

  it('given an editor that does not say when its text changes, when refresh is called, then the hook reads again', async () => {
    const { editor, fake } = mount(DEF_IF, { editor: (client) => ({ client: { ...client, on: () => () => undefined }, settled: () => Promise.resolve() }) });
    const { result } = renderHook(() => useBlockModel(editor));
    await waitFor(() => expect(result.current.status).toBe('ready'));

    act(() => {
      fake.userSelects(DEF_IF.length);
      fake.userTypes('q = 1');
      result.current.refresh();
    });

    await waitFor(() => expect(result.current.model?.text).toBe(`${DEF_IF}q = 1`));
  });

  it('given an editor without a tree, when the hook mounts, then the status is unavailable', async () => {
    const fake = createFakeEditor({ text: DEF_IF });
    const editor: BlockEditor = { client: createEditorClient(fake.engine), settled: () => Promise.resolve() };

    const { result } = renderHook(() => useBlockModel(editor));

    await waitFor(() => expect(result.current.status).toBe('unavailable'));
    expect(result.current.model).toBeNull();
  });
});
