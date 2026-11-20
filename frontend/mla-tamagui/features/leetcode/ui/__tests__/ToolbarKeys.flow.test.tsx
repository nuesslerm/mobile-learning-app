import {
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  screen,
  waitFor,
  type FlowCodeEditor,
  type FlowSession,
} from '@lib/flow-test-harness';

import { app } from '../../../../tests/flow/app';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import { useToolbarConfigStore } from '../../state/toolbarConfigStore';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';

// The net under every change to how the toolbar's keys reach the editor: each key is tapped on the real CodeMirror and what it
// leaves behind is asserted, then the toolbar's Undo. A case marked "pinned" records today's behavior, known quirk or not, so a
// later change to it is a decision with a commit message and not a side effect. A case marked "known gap" states what a key should do
// and fails today (it.fails), so the commit that fixes the gap has to flip it: the transform keys (Tab on a selection, Dedent, Comment,
// Newline, the wrap of a selection) send the whole document back through setText, and the page's reply to it sends the old text again
// before the new one settles, which loses the key's caret and its undo step.

const ACCEPTED: RunResponse = {
  source: 'backend-run',
  kind: 'test',
  state: 'SUCCESS',
  requestId: 'flow-request',
  run_success: true,
  status_msg: 'Accepted',
  total_correct: 5,
  total_testcases: 5,
};

const gateway: LeetCodeGatewayPort = {
  userStatus: vi.fn(async () => ({ isSignedIn: true, username: 'flow-user' })),
  test: vi.fn(async () => ({ ...ACCEPTED, interpret_id: 'flow-interpret' })),
  submit: vi.fn(async () => ACCEPTED),
  poll: vi.fn(async () => ACCEPTED),
  runLocal: vi.fn(async () => ACCEPTED),
  format: vi.fn(async (p: { lang: string; code: string }) => p.code),
  loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })),
};

// reset before the screen mounts: a store write while a screen is still mounted is an update outside act
beforeEach(() => {
  useRunnerSettingStore.setState({ runner: 'backend' });
  useToolbarConfigStore.getState().resetToDefault();
});
afterEach(() => useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER }));

// each key crosses the WebView twin as one or two messages, about a second on a loaded machine
const eventually = (check: () => void, timeout = 8_000) => waitFor(check, { timeout });

async function openEditor(): Promise<{ session: FlowSession; editor: FlowCodeEditor }> {
  const ports: LeetCodePortOverrides = {
    gateway,
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
  };
  const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
  return { session, editor: await session.editor() };
}

// The soft keyboard up, signalled the way the device harness does: that raises the toolbar.
async function raiseKeyboard(): Promise<void> {
  Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: true });
  window.dispatchEvent(new Event('mcp:keyboardframe'));
  await waitFor(() => expect(screen.queryByTestId('EditorSyntax-Paren')).not.toBeNull(), { timeout: 5_000 });
}

// The editor holds `code` with the selection set, the keyboard is up, and the screen has seen both.
async function prepare(code: string, start: number, end = start) {
  const opened = await openEditor();
  await opened.editor.setText(code);
  await opened.editor.select(start, end);
  await raiseKeyboard();
  await eventually(() => expect(opened.editor.text()).toBe(code));
  await eventually(() => expect(opened.editor.selection()).toEqual({ start, end }));
  return opened;
}

async function undoRestores(session: FlowSession, editor: FlowCodeEditor, code: string, timeout?: number) {
  await session.tap('EditorAction-Undo');
  await eventually(() => expect(editor.text()).toBe(code), timeout);
}

const caret = (at: number) => ({ start: at, end: at });

describe('the syntax toolbar on the real editor (indentation and lines)', () => {
  it('given a caret and no selection, when Tab is tapped, then one level goes in at the caret, the caret follows it, and Undo restores the text', async () => {
    // Given
    const { session, editor } = await prepare('x = 1', 0);
    // When
    await session.tap('EditorSyntax-Tab');
    // Then
    await eventually(() => expect(editor.text()).toBe('    x = 1'));
    await eventually(() => expect(editor.selection()).toEqual(caret(4)));
    await undoRestores(session, editor, 'x = 1');
  });

  it('given two lines selected, when Tab is tapped, then both are indented and the block stays selected', async () => {
    const { session, editor } = await prepare('a\nb', 0, 3);
    await session.tap('EditorSyntax-Tab');
    await eventually(() => expect(editor.text()).toBe('    a\n    b'));
    await eventually(() => expect(editor.selection()).toEqual({ start: 0, end: 11 }));
  });

  // known gap: the transform goes through setText, which leaves no undo step
  it.fails('given two lines selected and indented by Tab, when Undo is tapped, then the text before the key returns', async () => {
    const { session, editor } = await prepare('a\nb', 0, 3);
    await session.tap('EditorSyntax-Tab');
    await eventually(() => expect(editor.text()).toBe('    a\n    b'));
    await undoRestores(session, editor, 'a\nb', 2_000);
  });

  it('given an indented line and the caret after its text, when Dedent is tapped, then a level goes and the caret keeps its place in the text', async () => {
    const { session, editor } = await prepare('    x', 5);
    await session.tap('EditorSyntax-Dedent');
    await eventually(() => expect(editor.text()).toBe('x'));
    await eventually(() => expect(editor.selection()).toEqual(caret(1)));
  });

  // known gap: the transform goes through setText, which leaves no undo step
  it.fails('given an indented line dedented by Dedent, when Undo is tapped, then the text before the key returns', async () => {
    const { session, editor } = await prepare('    x', 5);
    await session.tap('EditorSyntax-Dedent');
    await eventually(() => expect(editor.text()).toBe('x'));
    await undoRestores(session, editor, '    x', 2_000);
  });

  it('given a plain line, when Comment is tapped twice, then it is commented and uncommented with the caret after the text each time', async () => {
    const { session, editor } = await prepare('x = 1', 5);
    await session.tap('EditorSyntax-Comment');
    await eventually(() => expect(editor.text()).toBe('# x = 1'));
    await eventually(() => expect(editor.selection()).toEqual(caret(7)));
    await session.tap('EditorSyntax-Comment');
    await eventually(() => expect(editor.text()).toBe('x = 1'));
    await eventually(() => expect(editor.selection()).toEqual(caret(5)));
  });

  // pinned, known quirk: the transform puts the caret at 2 (see the unit test of toggleComment), the key's own caret is lost and it stays at 0
  it('given the caret at the start of an indented line, when Comment is tapped, then the line is commented and the caret stays at 0 (pinned, known quirk)', async () => {
    const { session, editor } = await prepare('    x', 0);
    await session.tap('EditorSyntax-Comment');
    await eventually(() => expect(editor.text()).toBe('    # x'));
    await eventually(() => expect(editor.selection()).toEqual(caret(0)));
  });

  it('given a line that ends in a colon, when Newline is tapped, then the next line starts one level deeper', async () => {
    const { session, editor } = await prepare('if x:', 5);
    await session.tap('EditorSyntax-Newline');
    await eventually(() => expect(editor.text()).toBe('if x:\n    '));
    await eventually(() => expect(editor.selection()).toEqual(caret(10)));
  });

  // known gap: the transform goes through setText, which leaves no undo step
  it.fails('given a line broken by Newline, when Undo is tapped, then the text before the key returns', async () => {
    const { session, editor } = await prepare('if x:', 5);
    await session.tap('EditorSyntax-Newline');
    await eventually(() => expect(editor.text()).toBe('if x:\n    '));
    await undoRestores(session, editor, 'if x:', 2_000);
  });
});

describe('the syntax toolbar on the real editor (caret keys)', () => {
  it('given a caret in the middle, when Left and Right are tapped, then the caret moves one character each way', async () => {
    const { session, editor } = await prepare('abc', 2);
    await session.tap('EditorSyntax-Left');
    await eventually(() => expect(editor.selection()).toEqual(caret(1)));
    await session.tap('EditorSyntax-Right');
    await eventually(() => expect(editor.selection()).toEqual(caret(2)));
    expect(editor.text()).toBe('abc');
  });

  it('given a selection, when Left is tapped, then it collapses to the selection start', async () => {
    const { session, editor } = await prepare('abcdef', 2, 4);
    await session.tap('EditorSyntax-Left');
    await eventually(() => expect(editor.selection()).toEqual(caret(2)));
  });

  it('given the caret at the end, when Right is tapped, then it stays', async () => {
    const { session, editor } = await prepare('abc', 3);
    await session.tap('EditorSyntax-Right');
    await eventually(() => expect(editor.selection()).toEqual(caret(3)));
  });
});

describe('the syntax toolbar on the real editor (typed keys)', () => {
  it.each([
    ['Bracket', '[]'],
    ['Brace', '{}'],
    ['Quote', '""'],
    ['Apostrophe', "''"],
  ])('given the caret after "x = ", when %s is tapped, then the editor adds the pair "%s" with the caret between and Undo restores the text', async (id, pair) => {
    const { session, editor } = await prepare('x = ', 4);
    await session.tap(`EditorSyntax-${id}`);
    await eventually(() => expect(editor.text()).toBe(`x = ${pair}`));
    await eventually(() => expect(editor.selection()).toEqual(caret(5)));
    await undoRestores(session, editor, 'x = ');
  });

  it('given "abc" selected, when Parentheses is tapped, then it is wrapped', async () => {
    const { session, editor } = await prepare('abc', 0, 3);
    await session.tap('EditorSyntax-Paren');
    await eventually(() => expect(editor.text()).toBe('(abc)'));
  });

  // known gap: the wrap goes through setText, so the inner text does not stay selected and there is no undo step
  it.fails('given "abc" selected and wrapped by Parentheses, when the key has run, then the inner text stays selected', async () => {
    const { session, editor } = await prepare('abc', 0, 3);
    await session.tap('EditorSyntax-Paren');
    await eventually(() => expect(editor.text()).toBe('(abc)'));
    await eventually(() => expect(editor.selection()).toEqual({ start: 1, end: 4 }), 2_000);
  });

  it.fails('given "abc" selected and wrapped by Parentheses, when Undo is tapped, then the text before the key returns', async () => {
    const { session, editor } = await prepare('abc', 0, 3);
    await session.tap('EditorSyntax-Paren');
    await eventually(() => expect(editor.text()).toBe('(abc)'));
    await undoRestores(session, editor, 'abc', 2_000);
  });

  it.each([
    ['Equals', 'x='],
    ['Underscore', 'x_'],
    ['Dot', 'x.'],
  ])('given the caret after "x", when %s is tapped, then the character is typed there', async (id, expected) => {
    const { session, editor } = await prepare('x', 1);
    await session.tap(`EditorSyntax-${id}`);
    await eventually(() => expect(editor.text()).toBe(expected));
    await eventually(() => expect(editor.selection()).toEqual(caret(2)));
  });

  it('given a closing bracket after the caret, when the same pair key is tapped twice, then the second tap adds a second pair and never steps over the first (typed, not inserted)', async () => {
    const { session, editor } = await prepare('f', 1);
    await session.tap('EditorSyntax-Paren');
    await eventually(() => expect(editor.text()).toBe('f()'));
    await session.tap('EditorSyntax-Paren');
    await eventually(() => expect(editor.text()).toBe('f(())'));
    await eventually(() => expect(editor.selection()).toEqual(caret(3)));
  });
});

describe('the syntax toolbar on the real editor (custom keys)', () => {
  it('given a custom key that inserts "->", when it is tapped, then the text goes in at the caret and the caret follows it, and Undo restores the text', async () => {
    useToolbarConfigStore.getState().addCustom({ label: '->', insert: '->' });
    const { session, editor } = await prepare('x', 1);
    await session.tap('EditorSyntax-Custom');
    await eventually(() => expect(editor.text()).toBe('x->'));
    await eventually(() => expect(editor.selection()).toEqual(caret(3)));
    await undoRestores(session, editor, 'x');
  });

  it('given a custom key with a negative cursor offset, when it is tapped, then the caret sits that many characters before the end of the insert', async () => {
    useToolbarConfigStore.getState().addCustom({ label: '[]', insert: '[]', cursorOffset: -1 });
    const { session, editor } = await prepare('x', 1);
    await session.tap('EditorSyntax-Custom');
    await eventually(() => expect(editor.text()).toBe('x[]'));
    await eventually(() => expect(editor.selection()).toEqual(caret(2)));
  });

  // pinned, known quirk: the insert goes at the selection's head and leaves the selected text in place; Paste shares this path
  it('given text selected, when a custom key is tapped, then the insert goes in at the end of the selection and the selected text stays (pinned, known quirk)', async () => {
    useToolbarConfigStore.getState().addCustom({ label: '!', insert: '!' });
    const { session, editor } = await prepare('abc', 0, 3);
    await session.tap('EditorSyntax-Custom');
    await eventually(() => expect(editor.text()).toBe('abc!'));
    await eventually(() => expect(editor.selection()).toEqual(caret(4)));
  });
});
