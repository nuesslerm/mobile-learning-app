import { createEditorClient, unwrap, type Range } from '@lib/code-editor-api';
import { createFakeEditor } from '@lib/code-editor-api/testing';
import { describe, expect, it } from 'vitest';

import { clearEditMark, EDIT_MARK_ID, markEdit, setEditMarkHidden } from '../editMark';

const TEXT = 'def solve(xs):\n    total = 0\n    return total\n';

function editor(options: { marks?: boolean; instance?: string } = {}) {
  const fake = createFakeEditor({ text: TEXT, instance: options.instance ?? 'page-1', ...(options.marks === undefined ? {} : { marks: options.marks }) });
  const client = createEditorClient(fake.engine);
  // an edit that landed: the replace's own `landed` is the span in the text it produced, at the revision it produced
  const land = async (range: Range, text: string) => {
    const reply = await client.transact([{ op: 'replace', range, text }]);
    if (!reply.ok) throw new Error(`the edit was refused: ${reply.error.code}`);
    return { landed: reply.value.landed[0] as Range, revision: reply.revision };
  };
  const hide = async (hidden: boolean) => {
    const reply = await setEditMarkHidden(client, hidden);
    if (!reply?.ok) throw new Error('the edit mark could not be set');
  };
  const drawn = () => fake.peek().marks;
  const under = (range: Range) => fake.text().slice(range.from, range.to);
  return { fake, client, land, hide, drawn, under };
}

const FIRST = { from: 15, to: 28 };

describe('markEdit', () => {
  it('given an edit that landed, when it is marked, then the editor holds one visible edit mark over exactly the text the edit put there', async () => {
    const { client, land, drawn, under } = editor();
    const { landed, revision } = await land(FIRST, '    seen = {}');

    unwrap(await markEdit(client, landed, revision, 'page-1'));

    expect(drawn()).toEqual([{ id: EDIT_MARK_ID, kind: 'edit', range: landed }]);
    expect(under(landed)).toBe('    seen = {}');
  });

  it('given a keystroke typed after the edit landed, when the edit is marked at the revision it produced, then the mark covers the same text where the keystroke moved it', async () => {
    const { fake, client, land, drawn, under } = editor();
    const { landed, revision } = await land(FIRST, '    seen = {}');
    fake.userTypes('# ');

    unwrap(await markEdit(client, landed, revision, 'page-1'));

    const [mark] = drawn();
    expect(mark?.range).toEqual({ from: landed.from + 2, to: landed.to + 2 });
    expect(under(mark?.range as Range)).toBe('    seen = {}');
  });

  it('given a mark from an earlier edit, when a later edit is marked, then the one mark is over the later edit', async () => {
    const { client, land, drawn, under } = editor();
    const first = await land(FIRST, '    seen = {}');
    unwrap(await markEdit(client, first.landed, first.revision, 'page-1'));
    const second = await land({ from: 0, to: 3 }, 'fun');

    unwrap(await markEdit(client, second.landed, second.revision, 'page-1'));

    expect(drawn()).toHaveLength(1);
    expect(under(drawn()[0]?.range as Range)).toBe('fun');
  });

  it('given a revision of another editor instance, when an edit is marked, then it is refused and no mark is set', async () => {
    const { client, land, drawn } = editor();
    const { landed, revision } = await land(FIRST, '    seen = {}');

    const reply = await markEdit(client, landed, revision, 'page-0');

    expect(reply.ok).toBe(false);
    expect(!reply.ok && reply.error.code).toBe('instance-mismatch');
    expect(drawn()).toEqual([]);
  });

  it('given an editor that cannot draw marks, when an edit is marked, then the reply is unsupported and the text is untouched', async () => {
    const { fake, client, land } = editor({ marks: false });
    const { landed, revision } = await land(FIRST, '    seen = {}');
    const text = fake.text();

    const reply = await markEdit(client, landed, revision, 'page-1');

    expect(!reply.ok && reply.error.code).toBe('unsupported');
    expect(fake.text()).toBe(text);
  });
});

describe('setEditMarkHidden', () => {
  it('given an edit mark, when it is hidden, then the editor lists it and does not draw it', async () => {
    const { client, land, hide, drawn } = editor();
    const { landed, revision } = await land(FIRST, '    seen = {}');
    unwrap(await markEdit(client, landed, revision, 'page-1'));

    await hide(true);

    expect(drawn()).toEqual([{ id: EDIT_MARK_ID, kind: 'edit', range: landed, hidden: true }]);
    expect(unwrap(await client.marks()).marks).toEqual(drawn());
  });

  it('given a hidden edit mark and a keystroke typed before it, when it is shown, then it is drawn over the same text at its moved place', async () => {
    const { fake, client, land, hide, drawn, under } = editor();
    const { landed, revision } = await land(FIRST, '    seen = {}');
    unwrap(await markEdit(client, landed, revision, 'page-1'));
    await hide(true);
    fake.userTypes('# ');

    await hide(false);

    const [mark] = drawn();
    expect(mark?.hidden).toBeUndefined();
    expect(mark?.range).toEqual({ from: landed.from + 2, to: landed.to + 2 });
    expect(under(mark?.range as Range)).toBe('    seen = {}');
  });

  it('given no edit mark, when it is hidden, then nothing is set', async () => {
    const { client, drawn } = editor();

    expect(await setEditMarkHidden(client, true)).toBeNull();
    expect(drawn()).toEqual([]);
  });
});

describe('clearEditMark', () => {
  it('given an edit mark and another mark, when the edit mark is cleared, then only the other stays', async () => {
    const { client, land, drawn } = editor();
    const { landed, revision } = await land(FIRST, '    seen = {}');
    unwrap(await markEdit(client, landed, revision, 'page-1'));
    unwrap(await client.setMarks([{ id: 'run-error', kind: 'error', range: { from: 0, to: 3 } }]));

    unwrap(await clearEditMark(client));

    expect(drawn().map((mark) => mark.id)).toEqual(['run-error']);
  });
});
