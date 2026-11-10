// The real CodeMirror editor, reached through the web test driver its bundle exposes as
// `window.__editor__` (the same API the Playwright e2e tier uses). Every change goes through
// CodeMirror, so the app sees the bridge messages a user's typing would produce.
import { act, waitFor } from '@testing-library/react';

type EditorDriver = {
  getText(): string;
  getSelection(): { start: number; end: number } | null;
  setSelection(start: number, end: number): void;
  replaceSelection(text: string): void;
  setText(text: string): void;
};

export type FlowCodeEditor = {
  text(): string;
  selection(): { start: number; end: number } | null;
  type(text: string): Promise<void>;
  select(start: number, end?: number): Promise<void>;
  setText(text: string): Promise<void>;
  /** Text of each diff decoration CodeMirror is currently drawing. */
  diffRanges(): string[];
};

function frameDriver(container: HTMLElement): { driver: EditorDriver; doc: Document } | null {
  for (const frame of Array.from(container.ownerDocument.querySelectorAll('iframe'))) {
    const win = frame.contentWindow as (Window & { __editor__?: EditorDriver }) | null;
    const driver = win?.__editor__;
    if (driver && frame.contentDocument && driver.getText().length > 0) {
      return { driver, doc: frame.contentDocument };
    }
  }
  return null;
}

export async function mountedCodeEditor(container: HTMLElement): Promise<FlowCodeEditor> {
  let found: { driver: EditorDriver; doc: Document } | null = null;
  await waitFor(
    () => {
      found = frameDriver(container);
      if (!found) throw new Error('no hydrated CodeMirror editor mounted');
    },
    { timeout: 15_000 }
  );
  const { driver, doc } = found as unknown as { driver: EditorDriver; doc: Document };

  const change = async (mutate: () => void, done: () => boolean) => {
    await act(async () => {
      mutate();
    });
    await waitFor(() => {
      if (!done()) throw new Error('editor change not applied yet');
    });
  };

  return {
    text: () => driver.getText(),
    selection: () => driver.getSelection(),
    type: (text) => {
      const expected = driver.getText().length + text.length - selectionLength(driver);
      return change(
        () => driver.replaceSelection(text),
        () => driver.getText().length === expected
      );
    },
    select: (start, end = start) =>
      change(
        () => driver.setSelection(start, end),
        () => {
          const sel = driver.getSelection();
          return sel?.start === start && sel.end === end;
        }
      ),
    setText: (text) =>
      change(
        () => driver.setText(text),
        () => driver.getText() === text
      ),
    diffRanges: () =>
      Array.from(doc.querySelectorAll('.cm-diff-change'), (el) => el.textContent ?? ''),
  };
}

function selectionLength(driver: EditorDriver): number {
  const sel = driver.getSelection();
  return sel ? sel.end - sel.start : 0;
}
