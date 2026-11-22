import { createDeferredEngine, createEditorClient, type ApplyParams, type EditorEngine, type RequestName } from '@lib/code-editor-api';
import { createFakeEditor } from '@lib/code-editor-api/testing';
import { describe, expect, it } from 'vitest';

import { createCaretDrag, moveCaretBy } from '../caretDrag';
import { DEFAULT_TUNING, createCaretGesture } from '../cursorDrag';
import { stepChars, stepLines, stepWords } from '../cursorMotion';

const { CHAR_PX, LINE_PX, SLOP_PX } = DEFAULT_TUNING;

const LINE = 'alpha beta.gamma(delta, epsilon) zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau';
const LINES = 'first line\nsecond\nthird line is long\nfourth\nfifth line';

type Gate = { promise: Promise<void>; resolve: () => void };
const gate = (): Gate => {
  let resolve!: () => void;
  const promise = new Promise<void>((ok) => {
    resolve = ok;
  });
  return { promise, resolve };
};

/** The editor behind the same engine with a record of every request, and the reply of the first `apply` held until the test releases it. */
function watched(engine: EditorEngine, options: { holdFirstApply?: boolean } = {}) {
  const requests: { name: RequestName; params: unknown }[] = [];
  const applied = gate();
  const release = gate();
  const wrapped: EditorEngine = {
    listen: (types, onEvent) => engine.listen(types, onEvent),
    request: (async (name: RequestName, params: unknown) => {
      requests.push({ name, params });
      const reply = await engine.request(name, params as never);
      if (name === 'apply' && requests.filter((request) => request.name === 'apply').length === 1) {
        applied.resolve();
        if (options.holdFirstApply) await release.promise;
      }
      return reply;
    }) as EditorEngine['request'],
  };
  const applies = () => requests.filter((request) => request.name === 'apply').map((request) => (request.params as ApplyParams).steps);
  return { engine: wrapped, requests, applies, firstApplied: applied.promise, release: release.resolve };
}

// a gesture locked to the axis by a first sample on it, as the session's own first move does
const expectedHead = (text: string, anchor: number, head: number, axis: 'x' | 'y', last: { dx: number; dy: number }) => {
  const gesture = createCaretGesture({ text, anchor, head });
  gesture.move(axis === 'x' ? { dx: SLOP_PX, dy: 0 } : { dx: 0, dy: SLOP_PX });
  return gesture.move(last);
};

describe('createCaretDrag', () => {
  it('given a held first reply and thirty moves, when the reply is released, then the editor received exactly two select calls, the last at the newest target, and the selection is where the pure mapping says', async () => {
    const fake = createFakeEditor({ text: LINE });
    fake.userSelects(3);
    const held = watched(fake.engine, { holdFirstApply: true });
    const drag = createCaretDrag(createEditorClient(held.engine));

    drag.move({ dx: SLOP_PX, dy: 0 });
    await held.firstApplied;
    for (let step = 1; step <= 30; step += 1) drag.move({ dx: step * CHAR_PX, dy: 0 });
    held.release();
    const final = await drag.end();

    const target = expectedHead(LINE, 3, 3, 'x', { dx: 30 * CHAR_PX, dy: 0 });
    expect(target?.head).toBeGreaterThan(3);
    expect(held.applies()).toEqual([
      [{ op: 'select', anchor: 3, reveal: true }],
      [{ op: 'select', anchor: target?.head, reveal: true }],
    ]);
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: target?.head, head: target?.head });
    expect(final).toEqual({ start: target?.head, end: target?.head });
  });

  it('given a drag, when it lands, then the editor was read once and focused once, and the head was asked to be revealed', async () => {
    const fake = createFakeEditor({ text: LINE });
    const held = watched(fake.engine);
    const drag = createCaretDrag(createEditorClient(held.engine));

    drag.move({ dx: SLOP_PX, dy: 0 });
    drag.move({ dx: 5 * CHAR_PX, dy: 0 });
    await drag.end();

    expect(held.requests.map((request) => request.name)).toEqual(['view', 'read', 'apply', 'apply']);
    expect(held.requests.filter((request) => request.name === 'view')).toEqual([{ name: 'view', params: { focus: true } }]);
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 5, head: 5 });
    expect(fake.peek().revealed).toEqual({ from: 5, to: 5 });
  });

  it('given a selection at the grab, when the finger drags sideways, then the head moves and the anchor is unchanged', async () => {
    const fake = createFakeEditor({ text: LINE });
    fake.userSelects(2, 5);
    const drag = createCaretDrag(createEditorClient(fake.engine));

    drag.move({ dx: SLOP_PX, dy: 0 });
    drag.move({ dx: 3 * CHAR_PX, dy: 0 });
    const final = await drag.end();

    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 2, head: 8 });
    expect(final).toEqual({ start: 2, end: 8 });
  });

  it('given a selection whose head is before its anchor, when the finger drags down, then the head moves a line at the column of the grab and the anchor stays', async () => {
    const fake = createFakeEditor({ text: LINES });
    fake.userSelects(8, 3);
    const drag = createCaretDrag(createEditorClient(fake.engine));

    drag.move({ dx: 0, dy: SLOP_PX });
    drag.move({ dx: 0, dy: 2 * LINE_PX });
    await drag.end();

    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 8, head: stepLines(LINES, 3, 2) });
  });

  it('given a keystroke typed during the drag, when the next select is made, then it is refused as stale, the drag stops, and end resolves with what the editor holds', async () => {
    const fake = createFakeEditor({ text: LINE });
    const held = watched(fake.engine, { holdFirstApply: true });
    const drag = createCaretDrag(createEditorClient(held.engine));

    drag.move({ dx: 2 * CHAR_PX, dy: 0 });
    await held.firstApplied;
    fake.userTypes('k');
    drag.move({ dx: 6 * CHAR_PX, dy: 0 });
    held.release();
    const final = await drag.end();
    drag.move({ dx: 9 * CHAR_PX, dy: 0 });
    await drag.end();

    expect(held.applies()).toHaveLength(2);
    expect(fake.text()).toBe(`${LINE.slice(0, 2)}k${LINE.slice(2)}`);
    expect(final).toEqual({ start: 3, end: 3 });
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 3, head: 3 });
  });

  it('given an editor that is not ready, when a drag ends, then it resolves null and nothing throws', async () => {
    const drag = createCaretDrag(createEditorClient(createDeferredEngine(() => undefined)));

    drag.move({ dx: 40, dy: 0 });
    drag.move({ dx: 80, dy: 0 });

    await expect(drag.end()).resolves.toBeNull();
  });

  it('given a tap with no move, or a finger that never left the slop, when it ends, then nothing but focus was called and end resolves null', async () => {
    const fake = createFakeEditor({ text: LINE });
    const held = watched(fake.engine);
    const tap = createCaretDrag(createEditorClient(held.engine));
    const jitter = createCaretDrag(createEditorClient(held.engine));

    jitter.move({ dx: 1, dy: 2 });
    const results = [await tap.end(), await jitter.end()];

    expect(results).toEqual([null, null]);
    expect(held.requests).toEqual([
      { name: 'view', params: { focus: true } },
      { name: 'view', params: { focus: true } },
    ]);
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 0, head: 0 });
  });

  it('given a drag that is cancelled while a select is in flight, when the reply comes, then the waiting target is dropped, no further call is made and end resolves null', async () => {
    const fake = createFakeEditor({ text: LINE });
    const held = watched(fake.engine, { holdFirstApply: true });
    const drag = createCaretDrag(createEditorClient(held.engine));

    drag.move({ dx: 2 * CHAR_PX, dy: 0 });
    await held.firstApplied;
    drag.move({ dx: 8 * CHAR_PX, dy: 0 });
    drag.cancel();
    held.release();
    const final = await drag.end();

    expect(final).toBeNull();
    expect(held.applies()).toHaveLength(1);
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 2, head: 2 });
  });

  it('given a drag that is cancelled before the editor was read, when the read comes back, then no select is made', async () => {
    const fake = createFakeEditor({ text: LINE });
    const held = watched(fake.engine);
    const drag = createCaretDrag(createEditorClient(held.engine));

    drag.move({ dx: 5 * CHAR_PX, dy: 0 });
    drag.cancel();

    await expect(drag.end()).resolves.toBeNull();
    expect(held.applies()).toEqual([]);
  });

  it('given moves that arrive before the editor was read, when the read lands, then they are replayed in order so the axis is the one the first sample chose', async () => {
    const fake = createFakeEditor({ text: LINES });
    fake.userSelects(2);
    const drag = createCaretDrag(createEditorClient(fake.engine));

    drag.move({ dx: 1, dy: SLOP_PX + 4 });
    drag.move({ dx: 9 * CHAR_PX, dy: 3 * LINE_PX });
    await drag.end();

    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: stepLines(LINES, 2, 3), head: stepLines(LINES, 2, 3) });
  });

  it('given a drag that has ended, when more samples arrive, then they are ignored and end resolves the same selection', async () => {
    const fake = createFakeEditor({ text: LINE });
    const drag = createCaretDrag(createEditorClient(fake.engine));
    drag.move({ dx: 3 * CHAR_PX, dy: 0 });

    const first = await drag.end();
    drag.move({ dx: 9 * CHAR_PX, dy: 0 });
    const second = await drag.end();

    expect(second).toEqual(first);
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 3, head: 3 });
  });

  it('given a long drag, when the finger travels past the character zone, then the caret is on a word start', async () => {
    const fake = createFakeEditor({ text: LINE });
    const drag = createCaretDrag(createEditorClient(fake.engine));

    drag.move({ dx: SLOP_PX, dy: 0 });
    drag.move({ dx: 300, dy: 0 });
    const final = await drag.end();

    const head = final?.end as number;
    expect(head).toBeGreaterThan(stepChars(LINE, 0, DEFAULT_TUNING.CHAR_ZONE));
    expect(stepWords(LINE, stepWords(LINE, head, -1), 1)).toBe(head);
  });
});

describe('moveCaretBy', () => {
  it.each([
    ['one character right', LINES, 3, { unit: 'char', count: 1 } as const, 4],
    ['one character left', LINES, 3, { unit: 'char', count: -1 } as const, 2],
    ['one word right', 'foo.bar(baz)', 0, { unit: 'word', count: 1 } as const, 3],
    ['one word left', 'foo.bar(baz)', 8, { unit: 'word', count: -1 } as const, 7],
    ['one line down at the column', LINES, 8, { unit: 'line', count: 1 } as const, 17],
    ['one line up at the column', LINES, 20, { unit: 'line', count: -1 } as const, 13],
    ['the end of the text', 'ab', 2, { unit: 'char', count: 1 } as const, 2],
  ])('given a caret, when it is moved %s, then the editor holds the caret there with the keyboard kept up', async (_name, text, at, motion, expected) => {
    const fake = createFakeEditor({ text });
    fake.userSelects(at);
    const client = createEditorClient(fake.engine);

    const selection = await moveCaretBy(client, motion);

    expect(selection).toEqual({ start: expected, end: expected });
    expect(fake.peek()).toMatchObject({ selection: { ranges: [{ anchor: expected, head: expected }] }, focused: true, revealed: { from: expected, to: expected } });
  });

  it('given a selection, when it is moved by a word, then the head moves and the anchor stays', async () => {
    const fake = createFakeEditor({ text: 'foo bar baz' });
    fake.userSelects(1, 5);

    const selection = await moveCaretBy(createEditorClient(fake.engine), { unit: 'word', count: 1 });

    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 1, head: 8 });
    expect(selection).toEqual({ start: 1, end: 8 });
  });

  it('given an editor that is not ready, when a caret is moved, then it resolves null and nothing throws', async () => {
    await expect(moveCaretBy(createEditorClient(createDeferredEngine(() => undefined)), { unit: 'char', count: 1 })).resolves.toBeNull();
  });
});
