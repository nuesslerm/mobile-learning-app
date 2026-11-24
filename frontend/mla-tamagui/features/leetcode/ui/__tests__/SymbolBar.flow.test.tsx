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

import { DEFAULT_EDITOR_PREFERENCES } from '../../domain/editor-settings/editorPreferences';
import { resetGroups } from '../../domain/editor-settings/symbolGroups';
import { useEditorPreferencesStore } from '../../state/editor/editorPreferencesStore';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import { DEFAULT_RAIL_STYLE, DEFAULT_SYMBOL_BAR_SHOWN, useSymbolBarStore, type RailStyle } from '../../state/editor/symbolBarStore';
import { useToolbarConfigStore } from '../../state/toolbarConfigStore';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';

// The symbol rail above the keyboard on the real editor: the person's groups in their order, each key typed through the editor so
// brackets close themselves, a closer is stepped over and a selection is wrapped. The symbols rail is the default; the flow tests that press the
// classic character keys pin the classic rail.

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
  useEditorPreferencesStore.setState(DEFAULT_EDITOR_PREFERENCES);
  useSymbolBarStore.setState({ shown: DEFAULT_SYMBOL_BAR_SHOWN, groups: resetGroups(), railStyle: DEFAULT_RAIL_STYLE });
});
afterEach(() => useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER }));

// each key crosses the WebView twin as one or two messages, about a second on a loaded machine
const eventually = (check: () => void | Promise<void>, timeout = 8_000) => waitFor(check, { timeout });

async function openEditor(): Promise<{ session: FlowSession; editor: FlowCodeEditor }> {
  const ports: LeetCodePortOverrides = {
    gateway,
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
  };
  const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
  return { session, editor: await session.editor() };
}

// The soft keyboard up, signalled the way the device harness does: that raises the rails. `rail` is the test id the style's rail shows.
async function raiseKeyboard(rail: string): Promise<void> {
  Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: true });
  window.dispatchEvent(new Event('mcp:keyboardframe'));
  await waitFor(() => expect(screen.queryByTestId(rail)).not.toBeNull(), { timeout: 5_000 });
}

// The editor holds `code` with the selection set, the keyboard is up, and the screen has seen both.
async function prepare(code: string, start: number, end = start, railStyle: RailStyle = 'symbols') {
  useSymbolBarStore.setState({ railStyle });
  const opened = await openEditor();
  await opened.editor.setText(code);
  await opened.editor.select(start, end);
  await raiseKeyboard(railStyle === 'symbols' ? 'EditorSymbolBar' : 'EditorSyntaxToolbar');
  await eventually(() => expect(opened.editor.text()).toBe(code));
  await eventually(() => expect(opened.editor.selection()).toEqual({ start, end }));
  return opened;
}

const caret = (at: number) => ({ start: at, end: at });

const groupIds = () => screen.queryAllByTestId(/^SymbolBarGroup-/).map((group) => group.getAttribute('data-testid')?.replace('SymbolBarGroup-', ''));
const groupChars = (id: string) => screen.getByTestId(`SymbolBarGroup-${id}`).textContent;

describe('the symbol rail on the real editor (what it shows)', () => {
  it('given the symbols rail and the keyboard up, when the rail is read, then it holds Tab, the eight groups as the recording gives them and a key that hides the keyboard', async () => {
    await prepare('x', 1);

    expect(groupIds()).toEqual(['g1', 'g2', 'g3', 'g4', 'g5', 'g6', 'g7', 'g8']);
    expect(groupIds().map((id) => groupChars(id ?? ''))).toEqual(['#$^%', '&|?!', '\\@~_', '+-*/', '\'"`=', '{}[]', '()<>', '.:,;']);
    expect(screen.getByTestId('SymbolBarTab')).toBeTruthy();
    expect(screen.getByTestId('SymbolBarDismiss')).toBeTruthy();
    expect(screen.queryByTestId('EditorSyntaxToolbar')).toBeNull();
    expect(screen.getByTestId('EditorActionToolbar')).toBeTruthy();
  });

  it('given a group moved and one emptied in the store, when the rail is read, then it follows the new order and leaves the empty group out', async () => {
    useSymbolBarStore.getState().move(1, 2);
    useSymbolBarStore.getState().setGroup('g4', '');

    await prepare('x', 1);

    expect(groupIds()).toEqual(['g1', 'g3', 'g2', 'g5', 'g6', 'g7', 'g8']);
  });

  it('given a group of four characters, when the rail is read, then they sit as two rows of two', async () => {
    await prepare('x', 1);

    const rows = screen.getByTestId('SymbolBarGroup-g6').children;
    expect(Array.from(rows).map((row) => row.textContent)).toEqual(['{}', '[]']);
  });
});

describe('the symbol rail on the real editor (typing)', () => {
  it('given the caret after "x = ", when the { key is tapped, then the editor adds the pair with the caret between and Undo restores the text', async () => {
    const { session, editor } = await prepare('x = ', 4);

    await session.tap('SymbolBarKey-g6-0');

    await eventually(() => expect(editor.text()).toBe('x = {}'));
    await eventually(() => expect(editor.selection()).toEqual(caret(5)));
    await session.tap('EditorAction-Undo');
    await eventually(() => expect(editor.text()).toBe('x = '));
  });

  it('given "abc" selected, when the ( key is tapped, then it is wrapped and the inner text stays selected', async () => {
    const { session, editor } = await prepare('abc', 0, 3);

    await session.tap('SymbolBarKey-g7-0');

    await eventually(() => expect(editor.text()).toBe('(abc)'));
    await eventually(() => expect(editor.selection()).toEqual({ start: 1, end: 4 }));
  });

  it('given a closing bracket after the caret, when the matching closer key is tapped, then it steps over the closer and adds nothing', async () => {
    const { session, editor } = await prepare('f', 1);
    await session.tap('SymbolBarKey-g7-0');
    await eventually(() => expect(editor.text()).toBe('f()'));
    await eventually(() => expect(editor.selection()).toEqual(caret(2)));

    await session.tap('SymbolBarKey-g7-1');

    await eventually(() => expect(editor.selection()).toEqual(caret(3)));
    expect(editor.text()).toBe('f()');
  });

  it('given a plain symbol, when its key is tapped, then the character goes in at the caret', async () => {
    const { session, editor } = await prepare('x', 1);

    await session.tap('SymbolBarKey-g4-0');

    await eventually(() => expect(editor.text()).toBe('x+'));
    await eventually(() => expect(editor.selection()).toEqual(caret(2)));
  });

  it('given a group moved to the front, when its second key is tapped, then the rail shows it first and the key types its own character', async () => {
    useSymbolBarStore.getState().move(5, 0);
    const { session, editor } = await prepare('', 0);
    expect(groupIds()[0]).toBe('g6');

    await session.tap('SymbolBarKey-g6-1');

    await eventually(() => expect(editor.text()).toBe('}'));
  });

  it('given the caret at the start of a line, when Tab is tapped, then it runs the toolbar\'s Tab: one level goes in at the caret', async () => {
    const { session, editor } = await prepare('x = 1', 0);

    await session.tap('SymbolBarTab');

    await eventually(() => expect(editor.text()).toBe('    x = 1'));
    await eventually(() => expect(editor.selection()).toEqual(caret(4)));
  });

  it('given Tabs 2 in the preferences, when Tab is tapped, then two spaces go in', async () => {
    useEditorPreferencesStore.setState({ tabSize: 2 });
    const { session, editor } = await prepare('x', 0);

    await session.tap('SymbolBarTab');

    await eventually(() => expect(editor.text()).toBe('  x'));
  });
});

describe('the rail choice', () => {
  it('given the defaults, when the keyboard is up, then the classic syntax rail and the action rail show and the symbol rail does not', async () => {
    await prepare('x', 1, 1, 'classic');

    expect(screen.getByTestId('EditorSyntaxToolbar')).toBeTruthy();
    expect(screen.getByTestId('EditorActionToolbar')).toBeTruthy();
    expect(screen.queryByTestId('EditorSymbolBar')).toBeNull();
  });

  it.each<RailStyle>(['classic', 'symbols'])('given the switch is off with the %s style, when the keyboard is up, then neither syntax rail shows and the action rail stays', async (railStyle) => {
    useSymbolBarStore.setState({ shown: false, railStyle });
    const { editor } = await openEditor();
    await editor.setText('x');
    Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: true });
    window.dispatchEvent(new Event('mcp:keyboardframe'));

    await waitFor(() => expect(screen.queryByTestId('EditorActionToolbar')).not.toBeNull(), { timeout: 5_000 });

    expect(screen.queryByTestId('EditorSyntaxToolbar')).toBeNull();
    expect(screen.queryByTestId('EditorSymbolBar')).toBeNull();
    expect(screen.getByTestId('EditorAction-Done')).toBeTruthy();
  });
});
