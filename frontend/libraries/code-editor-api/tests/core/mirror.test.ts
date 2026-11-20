import { describe, expect, it } from 'vitest';

import { createDeferredEngine, createDocumentMirror, createEditorClient, unwrap } from '../../src';
import { createFakeEditor } from '../../src/testing';

const setup = async (text = '0123456789') => {
  const fake = createFakeEditor({ text });
  const client = createEditorClient(fake.engine);
  const mirror = await createDocumentMirror(fake.engine);
  return { fake, client, mirror };
};

describe('the document mirror', () => {
  it('given a held range, when text is inserted at its start and at its end, then the start moves it and the end stays outside', async () => {
    // Given a range over "345"
    const { client, mirror } = await setup();
    const held = mirror.track({ from: 3, to: 6 });

    // When text lands exactly at its start, then exactly at its end
    unwrap(await client.insertAt(3, 'AA'));
    expect(held.get()).toEqual({ from: 5, to: 8 });
    unwrap(await client.insertAt(8, 'BB'));

    // Then the range holds the same characters and no edit touched it
    expect(held.get()).toEqual({ from: 5, to: 8 });
    expect(mirror.text().slice(5, 8)).toBe('345');
    expect(held.touched()).toBe(false);
    expect(held.deleted()).toBe(false);
  });

  it('given a held range, when an edit lands strictly inside it or overlaps one side, then it is touched but not deleted', async () => {
    // Given two ranges over "345"
    const { client, mirror } = await setup();
    const inside = mirror.track({ from: 3, to: 6 });
    const overlapped = mirror.track({ from: 3, to: 6 });

    // When one edit lands inside the first and a replace overlaps the second's end
    unwrap(await client.insertAt(4, 'x'));
    expect(inside.touched()).toBe(true);
    unwrap(await client.replaceRange({ from: 6, to: 8 }, 'yy'));

    // Then both are touched and neither is deleted
    expect(overlapped.touched()).toBe(true);
    expect(inside.deleted()).toBe(false);
    expect(overlapped.deleted()).toBe(false);
  });

  it('given a held range, when a replace covers it, then it is deleted and collapses to a caret after the replacement', async () => {
    // Given a range over "45"
    const { client, mirror } = await setup();
    const held = mirror.track({ from: 4, to: 6 });

    // When a replace covers it
    unwrap(await client.replaceRange({ from: 3, to: 7 }, 'Z'));

    // Then it is flagged and no longer spans characters
    expect(held.deleted()).toBe(true);
    const range = held.get();
    expect(range?.from).toBe(range?.to);
  });

  it('given a lost event, when the next one arrives, then the mirror resyncs through one diff and held ranges follow it', async () => {
    // Given a mirror with a held range and an edit whose event was lost
    const { fake, client, mirror } = await setup();
    const held = mirror.track({ from: 5, to: 7 });
    const mirrorEvents: unknown[] = [];
    const stop = mirror.onChange(() => mirrorEvents.push(mirror.revision()));
    const before = mirror.revision();
    fake.userTypes('ab');
    unwrap(await client.insertAt(0, 'Q'));

    // When the gap is noticed and resynced
    await mirror.resync();

    // Then the text and revision equal a fresh read and the range moved with the single hunk
    const read = await client.getText();
    expect(mirror.text()).toBe(unwrap(read).text);
    expect(mirror.revision()).toBe(read.revision);
    expect(mirror.revision()).toBeGreaterThan(before);
    expect(mirror.text().slice(held.get()?.from, held.get()?.to)).toBe('56');
    stop();
  });

  it('given a held range and a new editor instance, when the mirror syncs, then the range reads null', async () => {
    // Given a mirror over a deferred engine bound to a first editor
    const deferred = createDeferredEngine(() => undefined);
    deferred.bind(createFakeEditor({ instance: 'one', text: '0123456789' }).engine);
    const mirror = await createDocumentMirror(deferred);
    const held = mirror.track({ from: 1, to: 2 });

    // When another editor instance is bound behind it
    deferred.bind(createFakeEditor({ instance: 'two', text: 'other' }).engine);
    await mirror.resync();

    // Then the old range is gone and the mirror shows the new document
    expect(held.get()).toBeNull();
    expect(mirror.instance()).toBe('two');
    expect(mirror.text()).toBe('other');
  });

  it('given a released range, when edits land, then it stops following', async () => {
    const { client, mirror } = await setup();
    const held = mirror.track({ from: 4, to: 6 });
    held.release();
    unwrap(await client.insertAt(0, 'AB'));
    expect(held.get()).toEqual({ from: 4, to: 6 });
  });
});
