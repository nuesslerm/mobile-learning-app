import { createDeferredEngine, createEditorClient } from '@lib/code-editor-api';
import { createFakeEditor } from '@lib/code-editor-api/testing';
import { describe, expect, it } from 'vitest';

import type { ToEditorMessage } from '../../../domain/ports';
import { createForwardingPost, everyMessageIsListed, FORWARDED_TYPES, LEGACY_TYPES } from '../editorForwarding';

// one sample of every message: a new ToEditorMessage type fails to compile here until it is given a sample
const SAMPLES: { [K in ToEditorMessage['type']]: Extract<ToEditorMessage, { type: K }> } = {
  init: { type: 'init', config: { initialText: '', isDark: false, fontSize: 14, lineHeight: 20, wordWrap: false, language: 'python', cursorColor: '#000', selectionColor: '#ccc', backgroundColor: '#fff', gutterColor: '#eee', borderColor: '#ddd', tabSize: 4, noAutoFocus: false } },
  updateConfig: { type: 'updateConfig', config: { wordWrap: true } },
  focus: { type: 'focus' },
  setSelection: { type: 'setSelection', start: 0, end: 0 },
  insertAt: { type: 'insertAt', text: 'x' },
  typeText: { type: 'typeText', text: 'x' },
  undo: { type: 'undo' },
  redo: { type: 'redo' },
  blur: { type: 'blur' },
  highlightRange: { type: 'highlightRange', start: 0, end: 1 },
  clearHighlights: { type: 'clearHighlights' },
  showDiff: { type: 'showDiff', start: 0, end: 1 },
  hideDiff: { type: 'hideDiff' },
};

function setup() {
  const fake = createFakeEditor({ text: 'abc' });
  const client = createEditorClient(fake.engine);
  const legacy: ToEditorMessage[] = [];
  return { fake, client, legacy, post: createForwardingPost(client, (msg) => legacy.push(msg)) };
}
const settled = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('the editor port forwarding', () => {
  it('given every message type, when each is listed, then it is forwarded or still legacy and never both', () => {
    expect(everyMessageIsListed).toBe(true);
    const listed = [...FORWARDED_TYPES, ...LEGACY_TYPES];
    expect(new Set(listed).size).toBe(listed.length);
    expect([...listed].sort()).toEqual(Object.keys(SAMPLES).sort());
  });

  it.each(LEGACY_TYPES)('given a %s message, when it is posted, then it takes the old script path and the engine is untouched', async (type) => {
    // Given an editor and a forwarding port
    const { fake, legacy, post } = setup();

    // When a legacy message is posted
    post(SAMPLES[type]);
    await settled();

    // Then it went to the old path and the editor did not change
    expect(legacy).toEqual([SAMPLES[type]]);
    expect(fake.peek().revision).toBe(0);
  });

  it('given an edit, when undo and redo are posted, then they run as typed calls and nothing goes to the old path', async () => {
    // Given an edit made through the API
    const { fake, client, legacy, post } = setup();
    await client.insertAt(0, 'Z');

    // When undo and then redo are posted
    post(SAMPLES.undo);
    await settled();
    expect(fake.text()).toBe('abc');
    post(SAMPLES.redo);
    await settled();

    // Then the text came back and the old path saw nothing
    expect(fake.text()).toBe('Zabc');
    expect(legacy).toEqual([]);
  });

  it('given a focus message, when it is posted, then the editor reports focused and the old path saw nothing', async () => {
    // Given a blurred editor and a forwarding port
    const { fake, legacy, post } = setup();
    expect(fake.peek().focused).toBe(false);

    // When focus is posted
    post(SAMPLES.focus);
    await settled();

    // Then the typed call focused it and nothing went to the old path
    expect(fake.peek().focused).toBe(true);
    expect(legacy).toEqual([]);
  });

  it('given a setSelection message, when it is posted, then the typed select lands, the editor is focused and the old path saw nothing', async () => {
    const { fake, legacy, post } = setup();

    post({ type: 'setSelection', start: 1, end: 2 });
    await settled();

    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 1, head: 2 });
    expect(fake.peek().focused).toBe(true);
    expect(legacy).toEqual([]);
  });

  it('given a setSelection past the end of the text, when it is posted, then nothing moves and the old script is not asked to throw', async () => {
    const { fake, legacy, post } = setup();

    post({ type: 'setSelection', start: 0, end: 99 });
    await settled();

    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 0, head: 0 });
    expect(legacy).toEqual([]);
  });

  it('given a caret, when insertAt is posted, then the text goes in there, the caret ends after it, and one undo takes it back', async () => {
    const { fake, client, legacy, post } = setup();
    await client.select(1);

    post({ type: 'insertAt', text: 'XY' });
    await settled();

    expect(fake.text()).toBe('aXYbc');
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 3, head: 3 });
    await client.undo();
    expect(fake.text()).toBe('abc');
    expect(legacy).toEqual([]);
  });

  it('given a negative offset, when insertAt is posted, then the caret sits that many characters before the end of the insert', async () => {
    const { fake, client, post } = setup();
    await client.select(3);

    post({ type: 'insertAt', text: '()', offset: -1 });
    await settled();

    expect(fake.text()).toBe('abc()');
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 4, head: 4 });
  });

  it('given a selection, when insertAt is posted, then the insert replaces it', async () => {
    const { fake, client, post } = setup();
    await client.select(0, 2);

    post({ type: 'insertAt', text: 'Z' });
    await settled();

    expect(fake.text()).toBe('Zc');
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 1, head: 1 });
  });

  it('given text with CRLF, when insertAt is posted, then it goes in as LF and the offset counts LF', async () => {
    const { fake, client, post } = setup();
    await client.select(3);

    post({ type: 'insertAt', text: 'a\r\nb', offset: -1 });
    await settled();

    expect(fake.text()).toBe('abca\nb');
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 5, head: 5 });
  });

  it('given a positive offset or one past the insert, when insertAt is posted, then the old script places the caret', async () => {
    const { fake, legacy, post } = setup();

    post({ type: 'insertAt', text: 'x', offset: 2 });
    post({ type: 'insertAt', text: 'x', offset: -2 });
    await settled();

    expect(legacy).toEqual([
      { type: 'insertAt', text: 'x', offset: 2 },
      { type: 'insertAt', text: 'x', offset: -2 },
    ]);
    expect(fake.text()).toBe('abc');
  });

  it('given no editor behind the engine yet, when insertAt is posted, then the old script gets it; a transport failure does not repeat an edit that may have landed', async () => {
    const legacy: ToEditorMessage[] = [];
    const post = createForwardingPost(createEditorClient(createDeferredEngine(() => undefined)), (msg) => legacy.push(msg));
    post({ type: 'insertAt', text: 'x' });
    await settled();
    expect(legacy).toEqual([{ type: 'insertAt', text: 'x' }]);

    const lost: ToEditorMessage[] = [];
    const closed = { request: () => Promise.reject(Object.assign(new Error('gone'), { code: 'closed' })), listen: () => () => undefined };
    createForwardingPost(createEditorClient(closed), (msg) => lost.push(msg))({ type: 'insertAt', text: 'x' });
    await settled();
    expect(lost).toEqual([]);
  });

  it('given a caret, when typeText is posted, then the keystroke is typed through the api as one undo step and the old path saw nothing', async () => {
    const { fake, client, legacy, post } = setup();
    await client.select(3);

    post({ type: 'typeText', text: ':' });
    await settled();

    expect(fake.text()).toBe('abc:');
    expect(fake.peek().selection.ranges[0]).toEqual({ anchor: 4, head: 4 });
    await client.undo();
    expect(fake.text()).toBe('abc');
    expect(legacy).toEqual([]);
  });

  it('given a text of more than a keystroke, a host that cannot type, or no editor yet, when typeText is posted, then the old script gets it', async () => {
    const { fake, legacy, post } = setup();
    const long = { type: 'typeText', text: 'x'.repeat(65) } as const;
    post(long);
    await settled();
    expect(legacy).toEqual([long]);
    expect(fake.text()).toBe('abc');

    const noType = createFakeEditor({ text: 'abc', typing: false });
    const viaNoType: ToEditorMessage[] = [];
    createForwardingPost(createEditorClient(noType.engine), (msg) => viaNoType.push(msg))({ type: 'typeText', text: '(' });
    const early: ToEditorMessage[] = [];
    createForwardingPost(createEditorClient(createDeferredEngine(() => undefined)), (msg) => early.push(msg))({ type: 'typeText', text: '(' });
    await settled();
    expect(viaNoType).toEqual([{ type: 'typeText', text: '(' }]);
    expect(early).toEqual([{ type: 'typeText', text: '(' }]);
    expect(noType.text()).toBe('abc');
  });

  it('given no editor behind the engine yet, when setSelection is posted, then the old script gets it', async () => {
    const legacy: ToEditorMessage[] = [];
    const post = createForwardingPost(createEditorClient(createDeferredEngine(() => undefined)), (msg) => legacy.push(msg));

    post({ type: 'setSelection', start: 1, end: 1 });
    await settled();

    expect(legacy).toEqual([{ type: 'setSelection', start: 1, end: 1 }]);
  });

  it('given no editor behind the engine yet, when focus is posted, then the old script gets it so the focus is not lost', async () => {
    // Given an engine with no page bound
    const legacy: ToEditorMessage[] = [];
    const post = createForwardingPost(createEditorClient(createDeferredEngine(() => undefined)), (msg) => legacy.push(msg));

    // When focus is posted
    post(SAMPLES.focus);
    await settled();

    // Then the typed call was refused as not ready and the old path took the message
    expect(legacy).toEqual([SAMPLES.focus]);
  });

  it('given the ten keys EditorInput sends, when updateConfig is posted, then it is applied through the typed API', async () => {
    const { client, legacy, post } = setup();
    post({ type: 'updateConfig', config: { wordWrap: true, isDark: true, fontSize: 16, lineHeight: 24, tabSize: 2, cursorColor: '#1', selectionColor: '#2', backgroundColor: '#3', gutterColor: '#4', borderColor: '#5' } });
    await settled();
    const config = await client.setConfig({});
    expect(config).toMatchObject({ ok: true, value: { wrap: true, fontSize: 16, tabSize: 2, palette: { dark: true, gutter: '#4' } } });
    expect(legacy).toEqual([]);
  });

  it('given an updateConfig with a key the typed API does not carry, when it is posted, then it stays on the old path', async () => {
    const { legacy, post } = setup();
    const message: ToEditorMessage = { type: 'updateConfig', config: { initialText: 'x', wordWrap: true } };
    post(message);
    await settled();
    expect(legacy).toEqual([message]);
  });
});
