// The real CodeMirror editor, reached through the typed editor API the page serves (`window.__editorEngine__`), the same surface the
// app's own toolbar, runner and voice edit use. A client makes the calls and a document mirror gives synchronous reads of the
// text and the selection. Focus and the diff decorations are not in the API, so those two read the iframe's DOM.
import {
  createDocumentMirror,
  createEditorClient,
  normalizeText,
  unwrap,
  type EditorClient,
  type EditorConfig,
  type EditorEngine,
  type EnclosingBlock,
  type SyntaxNode,
  type TreeParams,
  type TreeValue,
} from '@lib/code-editor-api';
import { act, waitFor } from '@testing-library/react';

export type FlowCodeEditor = {
  text(): string;
  selection(): { start: number; end: number } | null;
  type(text: string): Promise<void>;
  select(start: number, end?: number): Promise<void>;
  setText(text: string): Promise<void>;
  /** Puts the caret in the editor, as a tap on it does. */
  focus(): Promise<void>;
  /** Whether CodeMirror's content element holds the document's focus. */
  focused(): boolean;
  /** Text of each diff decoration CodeMirror is currently drawing. */
  diffRanges(): string[];
  /** The config the editor has now: the language it highlights and indents in, the tab size, the palette. */
  config(): Promise<EditorConfig>;
  /** The syntax tree the editor holds, as the typed API answers it. */
  tree(params?: TreeParams): Promise<TreeValue>;
  /** The innermost statement at `pos`, or null where the tree is not read. */
  statementAt(pos: number): Promise<SyntaxNode | null>;
  /** The block a caret at `pos` types into, from the tree and the mirror's text. */
  enclosingBlock(pos: number): Promise<EnclosingBlock | null>;
  /** A key CodeMirror's own keymap handles, pressed on its content element (not a toolbar key): the caller waits for the text to change. */
  press(key: 'Enter' | 'Tab' | 'Backspace'): Promise<void>;
};

const KEY_CODES = { Enter: 13, Tab: 9, Backspace: 8 } as const;

function frameEngine(container: HTMLElement): { engine: EditorEngine; doc: Document } | null {
  for (const frame of Array.from(container.ownerDocument.querySelectorAll('iframe'))) {
    const win = frame.contentWindow as (Window & { __editorEngine__?: EditorEngine }) | null;
    const engine = win?.__editorEngine__;
    if (engine && frame.contentDocument) return { engine, doc: frame.contentDocument };
  }
  return null;
}

export async function mountedCodeEditor(container: HTMLElement): Promise<FlowCodeEditor> {
  let found: { engine: EditorEngine; client: EditorClient; doc: Document } | null = null;
  await waitFor(
    async () => {
      const frame = frameEngine(container);
      if (!frame) throw new Error('no CodeMirror editor mounted');
      const client = createEditorClient(frame.engine);
      const hello = await client.hello();
      if (!hello.ok || !hello.value.ready) throw new Error('the editor api is not ready');
      const read = await client.getText();
      if (!read.ok || read.value.length === 0) throw new Error('the editor holds no text yet');
      found = { ...frame, client };
    },
    { timeout: 15_000 }
  );
  const { engine, client, doc } = found as unknown as { engine: EditorEngine; client: EditorClient; doc: Document };
  const mirror = await createDocumentMirror(engine);

  const change = async (mutate: () => Promise<unknown>, done: () => boolean) => {
    await act(async () => {
      await mutate();
    });
    await waitFor(() => {
      if (!done()) throw new Error('editor change not applied yet');
    });
  };

  const selection = () => {
    const state = mirror.selection();
    const range = state.ranges[state.main];
    return range ? { start: Math.min(range.anchor, range.head), end: Math.max(range.anchor, range.head) } : null;
  };
  const contentFocused = () => doc.activeElement?.classList.contains('cm-content') === true;

  return {
    text: () => mirror.text(),
    selection,
    type: async (text) => {
      const typed = normalizeText(text);
      const current = selection();
      const expected = mirror.text().length + typed.length - (current ? current.end - current.start : 0);
      // a plain insert at the selection with the caret after it, no bracket handling: what a paste does
      await change(
        async () =>
          unwrap(
            await client.transact([
              { op: 'replace', range: 'selection', text: typed },
              { op: 'select', anchor: { step: 0, edge: 'end' } },
            ])
          ),
        () => mirror.text().length === expected
      );
    },
    select: (start, end = start) =>
      change(
        async () => {
          unwrap(await client.select(start, end));
          await client.focus();
        },
        () => {
          const sel = selection();
          return sel?.start === start && sel.end === end;
        }
      ),
    setText: (text) =>
      change(
        async () => unwrap(await client.replaceRange({ from: 0, to: mirror.text().length }, text)),
        () => mirror.text() === normalizeText(text)
      ),
    focus: () =>
      change(
        async () => (doc.querySelector('.cm-content') as HTMLElement | null)?.focus(),
        contentFocused
      ),
    focused: contentFocused,
    diffRanges: () =>
      Array.from(doc.querySelectorAll('.cm-diff-change'), (el) => el.textContent ?? ''),
    config: async () => unwrap(await client.setConfig({})),
    tree: async (params) => unwrap(await client.tree(params)),
    statementAt: async (pos) => unwrap(await client.statementAt(pos)),
    enclosingBlock: async (pos) => unwrap(await client.enclosingBlock(pos, { text: mirror.text() })),
    press: async (key) => {
      const content = doc.querySelector('.cm-content');
      const win = doc.defaultView as (Window & typeof globalThis) | null;
      if (!content || !win) throw new Error('no CodeMirror content to press a key on');
      // built from the iframe's own constructor: CodeMirror ignores an event of the wrong window
      const event = new win.KeyboardEvent('keydown', { key, code: key, keyCode: KEY_CODES[key], bubbles: true, cancelable: true });
      await act(async () => {
        content.dispatchEvent(event);
      });
    },
  };
}
