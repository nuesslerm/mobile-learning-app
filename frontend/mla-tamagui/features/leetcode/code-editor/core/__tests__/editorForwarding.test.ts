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
  setText: { type: 'setText', text: 'x' },
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
