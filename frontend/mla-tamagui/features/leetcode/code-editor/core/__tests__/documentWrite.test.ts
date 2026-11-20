import { createEditorClient, unwrap } from '@lib/code-editor-api';
import { createFakeEditor } from '@lib/code-editor-api/testing';
import { describe, expect, it } from 'vitest';

import { planWrite } from '../documentWrite';

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
