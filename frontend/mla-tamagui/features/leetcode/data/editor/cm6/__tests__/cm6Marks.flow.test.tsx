import { createEditorClient, unwrap, type EditorClient, type MarkKind, type MarkSpec, type Range } from '@lib/code-editor-api';
import { mountedCodeEditor, waitFor, type FlowCodeEditor } from '@lib/flow-test-harness';
import { beforeAll, describe, expect, it, vi } from 'vitest';

import { mountPage } from './support/mountEditorPage';

/**
 * The marks of the typed API drawn by the real committed CodeMirror bundle, through the app's WebView twin. The contract suite proves the
 * marks and reveal rules on this same page (cm6Engine.contract.flow.test.tsx); this file proves what is drawn.
 * Not provable here: the scroll position (jsdom has no layout) and the tint's contrast (the browser e2e's golden).
 */
vi.setConfig({ testTimeout: 30_000, hookTimeout: 30_000 });

const CODE = ['def solve(xs):', '    total = 0', '    for x in xs:', '        total += x', '    return total', ''].join('\n');

// one page serves the file, as in the contract: every case loads its own text, which clears the marks
let page: Awaited<ReturnType<typeof mountPage>>;
let client: EditorClient;
let editor: FlowCodeEditor;

beforeAll(async () => {
  page = await mountPage({ init: true });
  client = createEditorClient(page.engine);
  await waitFor(async () => {
    const hello = await client.hello();
    if (!hello.ok || !hello.value.ready) throw new Error('the editor is not ready');
  });
  unwrap(await client.load(CODE));
  editor = await mountedCodeEditor(document.body);
});

const start = async (selection?: { anchor: number; head: number }) => {
  unwrap(await client.load(CODE, selection));
};

/** The text of line `n` (1-based) after its indentation: where a run's error is drawn. */
const contentOf = (n: number): Range => {
  const lines = CODE.split('\n');
  const from = lines.slice(0, n - 1).reduce((at, line) => at + line.length + 1, 0);
  const line = lines[n - 1] as string;
  return { from: from + (line.length - line.trimStart().length), to: from + line.length };
};
const mark = (id: string, range: Range, kind: MarkKind = 'error', hidden?: true): MarkSpec => ({ id, kind, range, ...(hidden ? { hidden } : {}) });
const rows = () => {
  const doc = page.frame()?.document;
  if (!doc) throw new Error('the page has no frame');
  return Array.from(doc.querySelectorAll('.cm-line.cm-run-error-line'), (row) => row.textContent ?? '');
};

describe('the real CodeMirror bundle drawing marks', () => {
  it('given the bundle, when the page says hello, then it announces marks and reveal', async () => {
    const hello = unwrap(await client.hello());

    expect(hello.capabilities.view).toEqual(['marks', 'reveal']);
  });

  it('given an error mark over line 3, when it is set, then the row is tinted and the text drawn as the error is the line without its indentation', async () => {
    await start();

    unwrap(await client.setMarks([mark('run-error', contentOf(3))]));

    expect(editor.errorLines()).toEqual(['for x in xs:']);
    expect(rows()).toEqual(['    for x in xs:']);
    expect(editor.diffRanges()).toEqual([]);
  });

  it('given an error mark from line 2 to line 3, when it is set, then both rows are tinted and the text drawn runs from the first character to the end of the second row', async () => {
    await start();

    unwrap(await client.setMarks([mark('run-error', { from: contentOf(2).from, to: contentOf(3).to })]));

    expect(rows()).toEqual(['    total = 0', '    for x in xs:']);
    expect(editor.errorLines()).toEqual(['total = 0', '    for x in xs:']);
  });

  it('given an empty error mark on a blank line, when it is set, then the row is tinted and nothing is underlined', async () => {
    await start();

    unwrap(await client.setMarks([mark('run-error', { from: CODE.length, to: CODE.length })]));

    expect(rows()).toEqual(['']);
    expect(editor.errorLines()).toEqual([]);
  });

  it('given an edit mark, when it is set, then the diff reader returns its text and no row is tinted', async () => {
    await start();

    unwrap(await client.setMarks([mark('diff', contentOf(5), 'edit')]));

    expect(editor.diffRanges()).toEqual(['return total']);
    expect(rows()).toEqual([]);
  });

  it('given a hidden mark, when it is set, then nothing is drawn and the api still lists it', async () => {
    await start();

    unwrap(await client.setMarks([mark('h', contentOf(3), 'error', true), mark('d', contentOf(5), 'edit', true)]));

    expect(editor.errorLines()).toEqual([]);
    expect(editor.diffRanges()).toEqual([]);
    expect(rows()).toEqual([]);
    expect(await editor.marks()).toMatchObject([{ id: 'h', hidden: true }, { id: 'd', hidden: true }]);
  });

  it('given a drawn mark, when the person types before it, then the drawn text is the same text at once, before any api call has run', async () => {
    await start();
    unwrap(await client.setMarks([mark('run-error', contentOf(3))]));
    const win = page.frame();

    win?.__editorDebug__?.simulateUser({ kind: 'select', anchor: 0 });
    win?.__editorDebug__?.simulateUser({ kind: 'type', text: '# a note\n' });

    expect(editor.errorLines()).toEqual(['for x in xs:']);
    expect(rows()).toEqual(['    for x in xs:']);
    const [held] = await editor.marks();
    expect(unwrap(await client.getText(held?.range)).text).toBe('for x in xs:');
  });

  it('given a drawn mark, when its text and a character on each side are replaced, then it is gone from the page and from the api', async () => {
    await start();
    const line = contentOf(3);
    unwrap(await client.setMarks([mark('run-error', line)]));

    unwrap(await client.replaceRange({ from: line.from - 1, to: line.to + 1 }, ' while True: '));

    expect(editor.errorLines()).toEqual([]);
    expect(rows()).toEqual([]);
    expect(await editor.marks()).toEqual([]);
  });

  it('given a drawn mark, when the next document is loaded, then nothing is drawn', async () => {
    await start();
    unwrap(await client.setMarks([mark('run-error', contentOf(3)), mark('diff', contentOf(5), 'edit')]));

    await start();

    expect(editor.errorLines()).toEqual([]);
    expect(editor.diffRanges()).toEqual([]);
    expect(rows()).toEqual([]);
    expect(await editor.marks()).toEqual([]);
  });

  it('given marks set and then cleared, when the page is read, then it draws nothing', async () => {
    await start();
    unwrap(await client.setMarks([mark('run-error', contentOf(3))]));

    unwrap(await client.clearMarks('all'));

    expect(rows()).toEqual([]);
    expect(editor.errorLines()).toEqual([]);
  });

  it('given a reveal of line 4, when view runs, then the revision, the text, the selection and the focus are as they were and the harness reads line 4', async () => {
    await start({ anchor: 3, head: 3 });
    const before = unwrap(await client.getText());
    const revision = (await client.getText()).revision;
    const focused = editor.focused();

    const reply = await client.reveal(contentOf(4));

    expect(unwrap(reply).focused).toBe(focused);
    expect(reply.revision).toBe(revision);
    expect(unwrap(await client.getText())).toEqual(before);
    expect(editor.focused()).toBe(focused);
    expect(await editor.revealedLine()).toBe(4);
  });

  it('given a reveal, when the person types, then the harness reads no line', async () => {
    await start();
    unwrap(await client.reveal(contentOf(4)));

    unwrap(await client.insertAt(0, 'x'));

    expect(await editor.revealedLine()).toBeNull();
  });
});
