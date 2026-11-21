// The real CodeMirror editor, reached through the typed editor API the page serves (`window.__editorEngine__`), the same surface the
// app's own toolbar, runner and voice edit use. A client makes the calls and a document mirror gives synchronous reads of the
// text and the selection. Focus and what the editor draws (diff and error marks) are read from the iframe's DOM.
import {
  createDocumentMirror,
  createEditorClient,
  normalizeText,
  unwrap,
  type ApplyValue,
  type EditorClient,
  type EditorConfig,
  type EditorEngine,
  type EnclosingBlock,
  type Mark,
  type Reply,
  type Step,
  type SyntaxNode,
  type TreeParams,
  type TreeValue,
} from '@lib/code-editor-api';
import { act, waitFor } from '@testing-library/react';

type ApplyOptions = NonNullable<Parameters<EditorClient['transact']>[1]>;

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
  /** The marks the typed API holds (head coordinates), hidden ones included. */
  marks(): Promise<readonly Mark[]>;
  /** Text of each error mark CodeMirror is currently drawing: what is underlined, not the whole row. */
  errorLines(): string[];
  /** The 1-based line of the last range the API was asked to reveal, null when none (an edit or a load clears it). Intent, not pixels: jsdom has no layout. */
  revealedLine(): Promise<number | null>;
  /** The config the editor has now: the language it highlights and indents in, the tab size, the palette. */
  config(): Promise<EditorConfig>;
  /** The syntax tree the editor holds, as the typed API answers it. */
  tree(params?: TreeParams): Promise<TreeValue>;
  /** The innermost statement at `pos`, or null where the tree is not read. */
  statementAt(pos: number): Promise<SyntaxNode | null>;
  /** The block a caret at `pos` types into, from the tree and the mirror's text. */
  enclosingBlock(pos: number): Promise<EnclosingBlock | null>;
  /** Types each character as a keystroke, through the editor's input handlers (an auto-closed bracket, a completion list opening). */
  keys(text: string): Promise<void>;
  /** Labels of the completion list the editor shows, first row first; empty when none is open. */
  completions(): string[];
  completionOpen(): boolean;
  /** Presses the row with this label, as a finger does, and waits for the list to go. */
  pickCompletion(label: string): Promise<void>;
  /** One call of steps through the typed API (a node step among them), answered as it is: a refusal is a reply to assert on, not a throw. A call that changed the text returns once the mirror has it; a preview or a refusal changes nothing, so it does not wait. */
  apply(steps: readonly Step[], options?: ApplyOptions): Promise<Reply<ApplyValue>>;
  /** Undoes the last change as the undo key does and waits for the mirror to have it; false when there was nothing to undo. */
  undo(): Promise<boolean>;
  /** A key CodeMirror's own keymap handles, pressed on its content element (not a toolbar key): the caller waits for the text to change. */
  press(key: 'Enter' | 'Tab' | 'Backspace'): Promise<void>;
};

const KEY_CODES = { Enter: 13, Tab: 9, Backspace: 8 } as const;
const MAX_TREE_BUDGET_MS = 1_000;

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

  // The tree is parsed against a time budget (50 ms unless a call says otherwise) and an answer that ran out of it says `complete: false`.
  // A read, and a write of the document, ask until the whole document is parsed, on the parse the last ask left, so what the editor does
  // next with its grammar (indent, a tree read) never depends on the machine's speed.
  const settled = async () => {
    for (;;) if (unwrap(await client.tree({ budgetMs: MAX_TREE_BUDGET_MS })).complete) return;
  };

  const selection = () => {
    const state = mirror.selection();
    const range = state.ranges[state.main];
    return range ? { start: Math.min(range.anchor, range.head), end: Math.max(range.anchor, range.head) } : null;
  };
  const contentFocused = () => doc.activeElement?.classList.contains('cm-content') === true;
  const completions = () => Array.from(doc.querySelectorAll('.cm-tooltip-autocomplete li[role=option]'), (li) => li.textContent ?? '');

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
      await settled();
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
    setText: async (text) => {
      await change(
        async () => unwrap(await client.replaceRange({ from: 0, to: mirror.text().length }, text)),
        () => mirror.text() === normalizeText(text)
      );
      await settled();
    },
    focus: () =>
      change(
        async () => (doc.querySelector('.cm-content') as HTMLElement | null)?.focus(),
        contentFocused
      ),
    focused: contentFocused,
    diffRanges: () =>
      Array.from(doc.querySelectorAll('.cm-diff-change'), (el) => el.textContent ?? ''),
    marks: async () => unwrap(await client.marks()).marks,
    errorLines: () => Array.from(doc.querySelectorAll('.cm-run-error'), (el) => el.textContent ?? ''),
    revealedLine: async () => {
      const { revealed } = unwrap(await client.marks());
      if (!revealed) return null;
      const { text } = unwrap(await client.getText());
      return text.slice(0, revealed.from).split('\n').length;
    },
    config: async () => unwrap(await client.setConfig({})),
    keys: async (text) => {
      for (const char of normalizeText(text)) {
        let revision = 0;
        await act(async () => {
          const reply = await client.transact([{ op: 'type', text: char }]);
          unwrap(reply);
          revision = reply.revision as number;
        });
        await waitFor(() => {
          if (mirror.revision() < revision) throw new Error('the keystroke has not reached the mirror yet');
        });
      }
    },
    apply: async (steps, options) => {
      const held: { reply?: Reply<ApplyValue> } = {};
      await act(async () => {
        held.reply = await client.transact(steps, options);
      });
      const reply = held.reply as Reply<ApplyValue>;
      const revision = reply.revision;
      if (reply.ok && options?.preview !== true && revision !== null) {
        await waitFor(() => {
          if (mirror.revision() < revision) throw new Error('the change has not reached the mirror yet');
        });
      }
      return reply;
    },
    undo: async () => {
      const done = { applied: false, revision: 0 };
      await act(async () => {
        const reply = await client.undo();
        done.applied = unwrap(reply).applied;
        done.revision = reply.revision as number;
      });
      if (done.applied) {
        await waitFor(() => {
          if (mirror.revision() < done.revision) throw new Error('the undo has not reached the mirror yet');
        });
      }
      return done.applied;
    },
    completions,
    completionOpen: () => completions().length > 0,
    pickCompletion: async (label) => {
      const row = Array.from(doc.querySelectorAll('.cm-tooltip-autocomplete li[role=option]')).find((li) => (li.textContent ?? '').startsWith(label)) as HTMLElement | undefined;
      if (!row) throw new Error(`the completion list has no row ${label}`);
      const win = doc.defaultView as (Window & typeof globalThis) | null;
      if (!win) throw new Error('no CodeMirror window to press a row in');
      // CodeMirror applies a completion on the row's mousedown, which is where a finger's tap lands first
      await act(async () => {
        row.dispatchEvent(new win.MouseEvent('mousedown', { bubbles: true, cancelable: true, button: 0 }));
      });
      await waitFor(() => {
        if (completions().length > 0) throw new Error('the completion list is still open');
      });
    },
    tree: async (params) => {
      if (params?.budgetMs === undefined) await settled();
      return unwrap(await client.tree(params));
    },
    statementAt: async (pos) => {
      await settled();
      return unwrap(await client.statementAt(pos));
    },
    enclosingBlock: async (pos) => {
      await settled();
      return unwrap(await client.enclosingBlock(pos, { text: mirror.text() }));
    },
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
