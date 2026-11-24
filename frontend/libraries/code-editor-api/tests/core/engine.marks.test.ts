import { describe, expect, it } from 'vitest';

import { createDeferredEngine, createEditorClient, unwrap, type Mark, type MarkSpec, type Range } from '../../src';
import { createFakeEditor } from '../../src/testing';

const spec = (id: string, from: number, to: number, hidden?: boolean): MarkSpec => ({ id, kind: 'error', range: { from, to }, ...(hidden === undefined ? {} : { hidden }) });

/** A fake editor whose host counts what the engine tells it, so a test can see when the engine pushes. */
function watched(text = '0123456789') {
  const fake = createFakeEditor({ text });
  const told: { marks: (readonly Mark[])[]; reveals: Range[] } = { marks: [], reveals: [] };
  const host = fake.host as { marks?: (list: readonly Mark[]) => void; reveal?: (range: Range) => void };
  const draw = host.marks;
  const scroll = host.reveal;
  host.marks = (list) => {
    told.marks.push(list);
    draw?.(list);
  };
  host.reveal = (range) => {
    told.reveals.push(range);
    scroll?.(range);
  };
  return { fake, told, client: createEditorClient(fake.engine) };
}

describe('what hello says about marks and reveal', () => {
  it('given a host with marks and reveal, when hello is read, then it lists both, and given a host without them, then neither and each call that needs one is unsupported naming it', async () => {
    const withBoth = createEditorClient(createFakeEditor({ text: 'abc' }).engine);
    const fake = createFakeEditor({ text: 'abc', marks: false });
    const without = createEditorClient(fake.engine);

    const helloWith = unwrap(await withBoth.hello());
    const helloWithout = unwrap(await without.hello());
    const marks = await without.setMarks([spec('a', 0, 1)]);
    const reveal = await without.reveal({ from: 0, to: 1 });
    const focus = await without.focus();

    expect(helloWith.capabilities.view).toEqual(['marks', 'reveal']);
    expect(helloWithout.capabilities.view).toEqual([]);
    expect(marks).toMatchObject({ ok: false, error: { code: 'unsupported', what: 'view:marks' } });
    expect(reveal).toMatchObject({ ok: false, error: { code: 'unsupported', what: 'view:reveal' } });
    expect(unwrap(focus)).toEqual({ focused: true, marks: [], revealed: null });
  });

  it('given a page with no editor yet, when hello is read, then it lists no view capability and a view call is not-ready', async () => {
    const deferred = createDeferredEngine(() => undefined);
    const client = createEditorClient(deferred);

    expect(unwrap(await client.hello()).capabilities.view).toEqual([]);
    expect(await client.marks()).toMatchObject({ ok: false, error: { code: 'not-ready' } });
  });
});

describe('when the engine tells the host about marks', () => {
  it('given marks set, a hidden one among them, when view runs, then the host is told the whole list once, and a query tells it nothing', async () => {
    const { fake, told, client } = watched();

    unwrap(await client.setMarks([spec('a', 1, 3), spec('h', 5, 6, true)]));
    unwrap(await client.marks());

    expect(told.marks).toHaveLength(1);
    expect(told.marks[0]).toEqual([{ id: 'a', kind: 'error', range: { from: 1, to: 3 } }, { id: 'h', kind: 'error', range: { from: 5, to: 6 }, hidden: true }]);
    expect(fake.peek().marks).toEqual(told.marks[0]);
  });

  it('given a mark and the person typing and undoing, when the text changes, then the host is told nothing and its own copy has followed the engine list', async () => {
    const { fake, told, client } = watched();
    unwrap(await client.setMarks([spec('a', 4, 6)]));
    fake.userSelects(0);

    fake.userTypes('ab');
    unwrap(await client.insertAt(0, 'cd'));
    unwrap(await client.undo());

    expect(told.marks).toHaveLength(1);
    expect(fake.peek().marks).toEqual(unwrap(await client.marks()).marks);
    expect(fake.peek().marks[0]?.range).toEqual({ from: 6, to: 8 });
  });

  it('given marks and a load, when the new text is shown, then the host is told an empty list once, and a load with no marks tells it nothing', async () => {
    const { fake, told, client } = watched();
    unwrap(await client.load('plain'));
    expect(told.marks).toEqual([]);
    unwrap(await client.setMarks([spec('a', 1, 3), spec('blank', 2, 2)]));

    unwrap(await client.load('fresh text'));

    expect(told.marks).toHaveLength(2);
    expect(told.marks[1]).toEqual([]);
    expect(fake.peek().marks).toEqual([]);
  });

  it('given a call that fails after its marks were checked, when it carries a focus and a reveal too, then the host is told nothing and the focus does not change', async () => {
    const { fake, told, client } = watched();

    const reply = await fake.engine.request('view', { focus: true, marks: { set: [spec('a', 1, 2), spec('b', 40, 41)] }, reveal: { from: 0, to: 1 } });

    expect(reply).toMatchObject({ ok: false, error: { code: 'out-of-range', step: 1 } });
    expect(told).toEqual({ marks: [], reveals: [] });
    expect(fake.peek()).toMatchObject({ focused: false, marks: [], revealed: null });
    expect(unwrap(await client.marks())).toMatchObject({ marks: [], revealed: null });
  });

  it('given a call that sets, reveals and focuses, when it succeeds, then the host got the marks, the range and the focus, and the reply says so', async () => {
    const { fake, told, client } = watched();

    const reply = unwrap(await client.setMarks([spec('run-error', 2, 5)], { reveal: { from: 2, to: 5 } }));
    unwrap(await client.focus());

    expect(reply).toEqual({ focused: false, marks: [{ id: 'run-error', kind: 'error', range: { from: 2, to: 5 } }], revealed: { from: 2, to: 5 } });
    expect(told.reveals).toEqual([{ from: 2, to: 5 }]);
    expect(fake.peek()).toMatchObject({ focused: true, revealed: { from: 2, to: 5 } });
  });

  it('given a reveal and then an edit, when view is queried, then revealed is null and the host was not told again', async () => {
    const { told, client } = watched();
    unwrap(await client.reveal({ from: 1, to: 2 }));

    unwrap(await client.insertAt(0, 'x'));

    expect(unwrap(await client.marks()).revealed).toBeNull();
    expect(told.reveals).toHaveLength(1);
  });

  it('given a clear that names no mark and an empty set, when view runs, then it is ok, the list is as it was and the host is told that list', async () => {
    const { told, client } = watched();
    unwrap(await client.setMarks([spec('a', 1, 2)]));

    const reply = unwrap(await client.clearMarks(['nothing-here']));

    expect(reply.marks).toHaveLength(1);
    expect(told.marks).toHaveLength(2);
  });
});

describe('a select step that reveals', () => {
  it('given a select step with reveal, when it lands, then view reports the new head as revealed without the host being asked to center it, and the next edit clears it', async () => {
    const { fake, told, client } = watched();

    unwrap(await client.select(2, 6, { reveal: true }));

    expect(unwrap(await client.marks()).revealed).toEqual({ from: 6, to: 6 });
    expect(fake.peek()).toMatchObject({ selection: { ranges: [{ anchor: 2, head: 6 }] }, revealed: { from: 6, to: 6 } });
    expect(told.reveals).toEqual([]);
    unwrap(await client.insertAt(0, 'x'));
    expect(unwrap(await client.marks()).revealed).toBeNull();
  });

  it('given a selection whose head is before its anchor, when it reveals, then the head is what is revealed', async () => {
    const { client } = watched();

    unwrap(await client.select(6, 2, { reveal: true }));

    expect(unwrap(await client.marks()).revealed).toEqual({ from: 2, to: 2 });
  });

  it('given an earlier reveal, when a select step without reveal runs, then revealed is as it was', async () => {
    const { client } = watched();
    unwrap(await client.reveal({ from: 8, to: 9 }));

    unwrap(await client.select(1));
    unwrap(await client.select(3, 4, { reveal: false }));

    expect(unwrap(await client.marks()).revealed).toEqual({ from: 8, to: 9 });
  });

  it('given a batch that edits and then selects with reveal, when it lands, then revealed is the head after the edit', async () => {
    const { client } = watched();

    unwrap(await client.transact([{ op: 'replace', range: { from: 0, to: 0 }, text: 'ab' }, { op: 'select', anchor: { step: 0, edge: 'end' }, reveal: true }]));

    expect(unwrap(await client.marks()).revealed).toEqual({ from: 2, to: 2 });
  });

  it('given a preview with reveal, when it runs, then nothing is selected or revealed', async () => {
    const { fake, client } = watched();

    unwrap(await client.select(4, undefined, { reveal: true, preview: true }));

    expect(unwrap(await client.marks()).revealed).toBeNull();
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 0, head: 0 });
  });

  it('given a host that cannot reveal, when a select step carries reveal, then it is unsupported naming view:reveal and the selection is where it was, and a select without reveal still lands', async () => {
    const fake = createFakeEditor({ text: '0123456789', marks: false });
    const client = createEditorClient(fake.engine);
    fake.userSelects(1);

    const refused = await client.select(5, undefined, { reveal: true });
    const plain = await client.select(5);

    expect(refused).toMatchObject({ ok: false, error: { code: 'unsupported', what: 'view:reveal' } });
    expect(plain.ok).toBe(true);
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 5, head: 5 });
  });

  it('given a select step with reveal that is refused as stale, when it runs, then nothing is revealed', async () => {
    const { fake, client } = watched();
    fake.userTypes('k');

    const reply = await client.select(3, undefined, { reveal: true, at: 0 });

    expect(reply).toMatchObject({ ok: false, error: { code: 'stale' } });
    expect(unwrap(await client.marks()).revealed).toBeNull();
  });
});
