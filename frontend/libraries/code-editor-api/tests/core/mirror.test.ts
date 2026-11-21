import { describe, expect, it } from 'vitest';

import { createDeferredEngine, createDocumentMirror, createEditorClient, unwrap, type EditorEngine } from '../../src';
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

  it('given a held range, when an edit lands strictly inside it, then it is touched and not deleted', async () => {
    const { client, mirror } = await setup();
    const held = mirror.track({ from: 3, to: 6 });

    unwrap(await client.insertAt(4, 'x'));

    expect(held.touched()).toBe(true);
    expect(held.deleted()).toBe(false);
    expect(held.get()).toEqual({ from: 3, to: 7 });
  });

  it('given a held range, when a replace overlaps only its end, then it is touched, not deleted, and keeps what is left of it', async () => {
    const { client, mirror } = await setup();
    const held = mirror.track({ from: 3, to: 6 });

    unwrap(await client.replaceRange({ from: 5, to: 8 }, 'yy'));

    expect(held.touched()).toBe(true);
    expect(held.deleted()).toBe(false);
    expect(mirror.text().slice(held.get()?.from, held.get()?.to)).toBe('34');
  });

  it('given a held range, when a replace covers it, then it is deleted and collapses to a caret after the replacement', async () => {
    // Given a range over "45"
    const { client, mirror } = await setup();
    const held = mirror.track({ from: 4, to: 6 });

    // When a replace covers it
    unwrap(await client.replaceRange({ from: 3, to: 7 }, 'Z'));

    // Then it is flagged and is a caret right after the replacement
    expect(held.deleted()).toBe(true);
    expect(held.get()).toEqual({ from: 4, to: 4 });
  });

  it('given a lost event, when the next one arrives, then the mirror resyncs through one diff and held ranges follow it', async () => {
    // Given a mirror whose engine loses the first changed event, and a range held over "56"
    const fake = createFakeEditor({ text: '0123456789' });
    let drop = false;
    const lossy: EditorEngine = {
      request: fake.engine.request,
      listen: (types, listener) => fake.engine.listen(types, (event) => (drop && event.type === 'changed' ? undefined : listener(event))),
    };
    const client = createEditorClient(fake.engine);
    const mirror = await createDocumentMirror(lossy);
    const held = mirror.track({ from: 5, to: 7 });

    // When an insert before it is lost, and a second one arrives
    drop = true;
    unwrap(await client.insertAt(0, 'Q'));
    drop = false;
    unwrap(await client.insertAt(0, 'RR'));
    await mirror.resync();

    // Then the mirror caught up by reading, and the held range moved by what both inserts added in front of it
    const read = unwrap(await client.getText());
    expect(mirror.text()).toBe(read.text);
    expect(mirror.text()).toBe('RRQ0123456789');
    expect(mirror.text().slice(held.get()?.from, held.get()?.to)).toBe('56');
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
