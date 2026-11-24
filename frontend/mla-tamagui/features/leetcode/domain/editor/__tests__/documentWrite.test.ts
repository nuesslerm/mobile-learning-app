import { createEditorClient, unwrap } from '@lib/code-editor-api';
import { createFakeEditor } from '@lib/code-editor-api/testing';
import { describe, expect, it } from 'vitest';

import { createDeferredEngine, type EditorClient } from '@lib/code-editor-api';

import { planWrite, writeThrough } from '../documentWrite';

async function write(current: string, next: string, selection?: { start: number; end: number }) {
  const fake = createFakeEditor({ text: current });
  const client = createEditorClient(fake.engine);
  const steps = planWrite(current, next, selection);
  if (steps.length > 0) unwrap(await client.transact(steps));
  return { fake, client, steps };
}

describe('planWrite', () => {
  it.each([
    ['an insert in the middle', 'abcd', 'abXYcd', { start: 4, end: 4 }],
    ['a delete', 'abcdef', 'abef', { start: 2, end: 2 }],
    ['a replace of the whole text', 'abc', 'xyz', { start: 3, end: 3 }],
    ['text from empty', '', 'abc', { start: 1, end: 3 }],
    ['text to empty', 'abc', '', { start: 0, end: 0 }],
    ['a caret before the hunk', 'abcdef', 'abcXef', { start: 1, end: 1 }],
    ['a caret after the hunk', 'abcdef', 'abXdef', { start: 6, end: 6 }],
    ['a selection that spans the hunk', 'abcdef', 'abXYZef', { start: 1, end: 6 }],
    ['a selection of the inserted text', 'abcdef', 'abXYZef', { start: 2, end: 5 }],
    ['a caret at the start of the hunk', 'abcdef', 'abXYZef', { start: 2, end: 2 }],
    ['a caret at the end of the hunk', 'abcdef', 'abXYZef', { start: 5, end: 5 }],
    ['a caret where the hunk is a pure delete', 'abcdef', 'abef', { start: 2, end: 2 }],
    ['a selection after a delete', 'abcdef', 'abef', { start: 2, end: 4 }],
    ['a block indented on every line', 'a\nb', '    a\n    b', { start: 0, end: 11 }],
  ])('given %s, when the plan is applied, then the text is the new text and the selection is the wanted one', async (_name, current, next, wanted) => {
    const { fake } = await write(current, next, wanted);
    expect(fake.text()).toBe(next);
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: wanted.start, head: wanted.end });
  });

  it('given identical text and a selection, when it is planned, then there is no replace and only the selection moves', async () => {
    const { fake, steps } = await write('abc', 'abc', { start: 1, end: 2 });
    expect(steps).toEqual([{ op: 'select', anchor: 1, head: 2 }]);
    expect(fake.peek().revision).toBe(0);
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 1, head: 2 });
  });

  it('given identical text and no selection, when it is planned, then there is nothing to do', () => {
    expect(planWrite('abc', 'abc')).toEqual([]);
  });

  it('given text with CRLF, when it is planned, then it is the LF text that is written', async () => {
    const { fake } = await write('a', 'a\r\nb', { start: 3, end: 3 });
    expect(fake.text()).toBe('a\nb');
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 3, head: 3 });
  });

  it('given a selection past the end of the new text, when it is planned, then it is held at the end', async () => {
    const { fake } = await write('abc', 'ab', { start: 9, end: 9 });
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 2, head: 2 });
  });

  it('given a write, when it is applied, then it is one undo step and one revision', async () => {
    const { fake, client } = await write('abcdef', 'abXYZef', { start: 5, end: 5 });
    expect(fake.peek().revision).toBe(1);
    unwrap(await client.undo());
    expect(fake.text()).toBe('abcdef');
  });
});

describe('writeThrough', () => {
  // a client whose getText hands back what the editor held and then lets the person type, so the call made at that revision is stale
  const typingAfterRead = (fake: ReturnType<typeof createFakeEditor>, windows: number): EditorClient => {
    const client = createEditorClient(fake.engine);
    let left = windows;
    return {
      ...client,
      getText: async (range) => {
        const reply = await client.getText(range);
        if (left > 0) {
          left -= 1;
          fake.userTypes('k');
        }
        return reply;
      },
    };
  };
  const toUpper = { label: 'test', next: (current: { text: string }) => ({ text: current.text.toUpperCase() }) };

  it('given an editor nobody types into, when a text is written, then it lands in one attempt as one undo step', async () => {
    const fake = createFakeEditor({ text: 'abc' });
    const client = createEditorClient(fake.engine);

    const outcome = await writeThrough(client, toUpper);

    expect(outcome).toEqual({ landed: true, text: 'ABC' });
    expect(fake.text()).toBe('ABC');
    unwrap(await client.undo());
    expect(fake.text()).toBe('abc');
  });

  it('given a keystroke after the read, when a text is written, then the call is stale, the write is planned again on the new text and lands', async () => {
    const fake = createFakeEditor({ text: 'abc' });

    const outcome = await writeThrough(typingAfterRead(fake, 1), toUpper);

    // the keystroke is kept and the write was planned against it
    expect(fake.text()).toBe('KABC');
    expect(outcome).toEqual({ landed: true, text: 'KABC' });
  });

  it('given a keystroke after every read, when a text is written, then it gives up after three tries and reports the text the editor holds now, not the text of the last read', async () => {
    const fake = createFakeEditor({ text: 'abc' });

    const outcome = await writeThrough(typingAfterRead(fake, 3), toUpper);

    expect(outcome).toEqual({ landed: false, code: 'stale', editorText: 'kkkabc' });
    expect(fake.text()).toBe('kkkabc');
  });

  it('given an editor that cannot be read, when a text is written, then nothing is written and there is no editor text to report', async () => {
    const client = createEditorClient(createDeferredEngine(() => undefined));

    const outcome = await writeThrough(client, toUpper);

    expect(outcome).toEqual({ landed: false, code: 'not-ready', editorText: null });
  });

  it('given the text the editor already holds, when it is written, then nothing is applied and the write counts as landed', async () => {
    const fake = createFakeEditor({ text: 'ABC' });

    const outcome = await writeThrough(createEditorClient(fake.engine), toUpper);

    expect(outcome).toEqual({ landed: true, text: 'ABC' });
    expect(fake.peek().revision).toBe(0);
  });

  it('given the editor goes away between attempts, when a text is written, then the write stops without a verdict', async () => {
    const fake = createFakeEditor({ text: 'abc' });
    let live = true;

    const outcome = await writeThrough(typingAfterRead(fake, 9), toUpper, () => {
      const answer = live;
      live = false;
      return answer;
    });

    expect(outcome).toBeNull();
  });
});
