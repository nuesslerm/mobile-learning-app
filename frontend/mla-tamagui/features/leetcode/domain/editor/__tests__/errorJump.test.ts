import { createDeferredEngine, createEditorClient, unwrap, type CallTrace, type EditorEngine, type Range } from '@lib/code-editor-api';
import { createFakeEditor } from '@lib/code-editor-api/testing';
import { describe, expect, it } from 'vitest';

import { checkpointOf, checkpointWithin, clearErrorLine, ERROR_MARK_ID, errorLineRange, showErrorLine } from '../errorJump';

const FIVE_LINES = ['class Solution:', '    def twoSum(self, nums, target):', '        seen = {}', '        return seen[target]', '        pass'].join('\n');

function editor(text: string, options: { instance?: string; marks?: boolean } = {}) {
  const calls: CallTrace[] = [];
  const fake = createFakeEditor({ text, ...options });
  const client = createEditorClient(fake.engine, { trace: (call) => calls.push(call) });
  return { fake, client, calls };
}

async function checkpointed(text: string, options: { instance?: string; marks?: boolean } = {}) {
  const made = editor(text, options);
  const checkpoint = await checkpointOf(made.client);
  if (!checkpoint) throw new Error('the fake editor gave no checkpoint');
  made.calls.length = 0;
  return { ...made, checkpoint };
}

const textOf = (text: string, range: Range) => text.slice(range.from, range.to);

describe('errorLineRange', () => {
  it('given code of five lines, when line 3 is asked, then the range is that line after its indentation', () => {
    const range = errorLineRange(FIVE_LINES, 3);

    expect(range && textOf(FIVE_LINES, range)).toBe('seen = {}');
  });

  it('given code of five lines, when the first and the last line are asked, then each is found, the last with no trailing newline', () => {
    const first = errorLineRange(FIVE_LINES, 1);
    const last = errorLineRange(FIVE_LINES, 5);

    expect(first && textOf(FIVE_LINES, first)).toBe('class Solution:');
    expect(last && textOf(FIVE_LINES, last)).toBe('pass');
    expect(last?.to).toBe(FIVE_LINES.length);
  });

  it.each([[0], [-2], [6], [1.5], [Number.NaN]])('given code of five lines, when line %s is asked, then there is no range', (line) => {
    expect(errorLineRange(FIVE_LINES, line)).toBeNull();
  });

  it('given a blank line and a line of spaces, when they are asked, then each is an empty range at the end of that line', () => {
    const text = 'a\n\n   \nb';

    expect(errorLineRange(text, 2)).toEqual({ from: 2, to: 2 });
    expect(errorLineRange(text, 3)).toEqual({ from: 6, to: 6 });
  });

  it('given code that ends with a newline, when the line after the last statement is asked, then it is the empty line at the end', () => {
    expect(errorLineRange('x = 1\n', 2)).toEqual({ from: 6, to: 6 });
    expect(errorLineRange('x = 1\n', 3)).toBeNull();
  });

  it('given an empty text, when line 1 is asked, then it is the empty range at 0', () => {
    expect(errorLineRange('', 1)).toEqual({ from: 0, to: 0 });
  });
});

describe('checkpointOf', () => {
  it('given an editor the person has typed in, when the checkpoint is taken, then it holds the instance, the revision and the text of that one read', async () => {
    const { fake, client } = editor('abc', { instance: 'page-7' });
    fake.userTypes('x');

    expect(await checkpointOf(client)).toEqual({ text: 'xabc', revision: 1, instance: 'page-7' });
  });

  it('given an engine with no editor behind it, when the checkpoint is taken, then there is none', async () => {
    const client = createEditorClient(createDeferredEngine(() => undefined));

    expect(await checkpointOf(client)).toBeNull();
  });

  it('given an editor, when the checkpoint is taken, then hello and the read are asked together before either is answered', async () => {
    const asked: string[] = [];
    const fake = createFakeEditor({ text: 'abc' });
    const engine: EditorEngine = {
      request: (name, params) => {
        asked.push(name);
        return fake.engine.request(name, params);
      },
      listen: (types, onEvent) => fake.engine.listen(types, onEvent),
    };

    await checkpointOf(createEditorClient(engine));

    expect(asked).toEqual(['hello', 'read']);
  });
});

describe('checkpointWithin', () => {
  it('given an editor that answers, when the checkpoint is taken within a generous deadline, then it is the checkpoint', async () => {
    const { client } = editor('abc');

    expect(await checkpointWithin(client, 60_000)).toMatchObject({ text: 'abc', revision: 0 });
  });

  it('given an editor that never answers, when the deadline passes, then there is no checkpoint', async () => {
    const silent: EditorEngine = { request: () => new Promise(() => undefined), listen: () => () => undefined };

    expect(await checkpointWithin(createEditorClient(silent), 20)).toBeNull();
  });

  it('given a write that is still queued, when the checkpoint is taken after it, then the checkpoint holds the written text', async () => {
    const { fake, client } = editor('abc');
    const queued = Promise.resolve().then(() => fake.userTypes('x'));

    expect(await checkpointWithin(client, 60_000, queued)).toMatchObject({ text: 'xabc', revision: 1 });
  });
});

describe('showErrorLine', () => {
  it('given a fake editor holding the code, when line 4 is shown, then one error mark covers that line, it is revealed, and the text, revision, selection and focus did not move', async () => {
    const { fake, client, checkpoint } = await checkpointed(FIVE_LINES);
    fake.userSelects(3, 9);
    const before = fake.peek();

    const jump = await showErrorLine(client, checkpoint, { line: 4 });

    const after = fake.peek();
    const range = errorLineRange(FIVE_LINES, 4) as Range;
    expect(jump).toEqual({ kind: 'shown', line: 4, range });
    expect(textOf(after.text, range)).toBe('return seen[target]');
    expect(after.marks).toEqual([{ id: ERROR_MARK_ID, kind: 'error', range }]);
    expect(after.revealed).toEqual(range);
    expect({ ...after, marks: [], revealed: null }).toEqual({ ...before, marks: [], revealed: null });
  });

  it('given no location, when it is shown, then the outcome is no-location and the editor was not called', async () => {
    const { fake, client, calls, checkpoint } = await checkpointed(FIVE_LINES);

    expect(await showErrorLine(client, checkpoint, undefined)).toEqual({ kind: 'no-location' });
    expect(await showErrorLine(client, checkpoint, null)).toEqual({ kind: 'no-location' });

    expect(calls).toEqual([]);
    expect(fake.peek().marks).toEqual([]);
  });

  it.each([[0], [6], [40]])('given a line the code does not have (%s), when it is shown, then the outcome is outside, nothing is marked and the editor was not called', async (line) => {
    const { fake, client, calls, checkpoint } = await checkpointed(FIVE_LINES);

    expect(await showErrorLine(client, checkpoint, { line })).toEqual({ kind: 'outside', line });

    expect(calls).toEqual([]);
    expect(fake.peek().marks).toEqual([]);
    expect(fake.peek().revealed).toBeNull();
  });

  it('given the person typed before line 4 after the checkpoint, when it is shown, then the mark is on the shifted line 4', async () => {
    const { fake, client, checkpoint } = await checkpointed(FIVE_LINES);
    fake.userTypes('# note\n');

    const jump = await showErrorLine(client, checkpoint, { line: 4 });

    const marks = fake.peek().marks;
    expect(jump.kind).toBe('shown');
    expect(marks).toHaveLength(1);
    expect(textOf(fake.text(), marks[0]?.range as Range)).toBe('return seen[target]');
  });

  it('given the person typed and removed a character after the checkpoint (the revision moved, the text is equal), when it is shown, then line 4 is marked', async () => {
    const { fake, client, checkpoint } = await checkpointed(FIVE_LINES);
    fake.userSelects(0);
    fake.userTypes('x');
    unwrap(await client.undo());
    expect(fake.text()).toBe(FIVE_LINES);
    expect(fake.peek().revision).toBe(checkpoint.revision + 2);

    const jump = await showErrorLine(client, checkpoint, { line: 4 });

    expect(jump.kind).toBe('shown');
    expect(textOf(fake.text(), fake.peek().marks[0]?.range as Range)).toBe('return seen[target]');
  });

  it("given line 4's text was deleted after the checkpoint, when it is shown, then the outcome is gone and nothing is marked or revealed", async () => {
    const { fake, client, checkpoint } = await checkpointed(FIVE_LINES);
    const line = errorLineRange(FIVE_LINES, 4) as Range;
    fake.userSelects(line.from, line.to);
    fake.userTypes('');

    const jump = await showErrorLine(client, checkpoint, { line: 4 });

    expect(jump).toEqual({ kind: 'gone', line: 4 });
    expect(fake.peek().marks).toEqual([]);
    expect(fake.peek().revealed).toBeNull();
  });

  it('given the fake editor was replaced by a new instance, when it is shown, then the outcome is refused with instance-mismatch and nothing is marked', async () => {
    const { checkpoint } = await checkpointed(FIVE_LINES, { instance: 'page-1' });
    const restarted = editor(FIVE_LINES, { instance: 'page-2' });

    const jump = await showErrorLine(restarted.client, checkpoint, { line: 4 });

    expect(jump).toEqual({ kind: 'refused', code: 'instance-mismatch' });
    expect(restarted.fake.peek().marks).toEqual([]);
  });

  it('given a fake editor without marks, when it is shown, then the outcome is refused with unsupported', async () => {
    const { fake, client, checkpoint } = await checkpointed(FIVE_LINES, { marks: false });

    expect(await showErrorLine(client, checkpoint, { line: 4 })).toEqual({ kind: 'refused', code: 'unsupported' });
    expect(fake.peek().marks).toEqual([]);
  });

  it('given a mark on line 4, when line 2 is shown from a later checkpoint, then exactly one error mark exists, on line 2', async () => {
    const { fake, client, checkpoint } = await checkpointed(FIVE_LINES);
    await showErrorLine(client, checkpoint, { line: 4 });

    await showErrorLine(client, checkpoint, { line: 2 });

    const marks = fake.peek().marks;
    expect(marks).toHaveLength(1);
    expect(textOf(fake.text(), marks[0]?.range as Range)).toBe('def twoSum(self, nums, target):');
  });

  it('given a blank line, when it is shown, then the mark is the empty range at it and it is revealed', async () => {
    const text = 'a = 1\n\nb = 2';
    const { fake, client, checkpoint } = await checkpointed(text);

    const jump = await showErrorLine(client, checkpoint, { line: 2 });

    expect(jump).toEqual({ kind: 'shown', line: 2, range: { from: 6, to: 6 } });
    expect(fake.peek().revealed).toEqual({ from: 6, to: 6 });
  });
});

describe('clearErrorLine', () => {
  it('given an error mark is shown, when it is cleared, then the editor lists no marks', async () => {
    const { fake, client, checkpoint } = await checkpointed(FIVE_LINES);
    await showErrorLine(client, checkpoint, { line: 4 });

    expect(await clearErrorLine(client)).toBeNull();

    expect(fake.peek().marks).toEqual([]);
  });

  it('given an error mark and an edit mark, when the error mark is cleared, then the edit mark stays', async () => {
    const { fake, client, checkpoint } = await checkpointed(FIVE_LINES);
    unwrap(await client.setMarks([{ id: 'edit-1', kind: 'edit', range: { from: 0, to: 5 } }]));
    await showErrorLine(client, checkpoint, { line: 4 });

    await clearErrorLine(client);

    expect(fake.peek().marks).toEqual([{ id: 'edit-1', kind: 'edit', range: { from: 0, to: 5 } }]);
  });

  it('given no mark, when it is cleared, then nothing is wrong; given an editor without marks, then the refusal code is returned', async () => {
    const { client } = editor(FIVE_LINES);
    const bare = editor(FIVE_LINES, { marks: false });

    expect(await clearErrorLine(client)).toBeNull();
    expect(await clearErrorLine(bare.client)).toBe('unsupported');
  });
});
