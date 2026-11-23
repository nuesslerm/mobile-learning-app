import {
  act,
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  screen,
  setStackHeadersRendered,
  userEvent,
  waitFor,
  within,
  type FlowCodeEditor,
  type FlowSession,
} from '@lib/flow-test-harness';

import { app } from '../../../../tests/flow/app';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { DEFAULT_EDITOR_PREFERENCES } from '../../domain/editor-settings/editorPreferences';
import { resetGroups } from '../../domain/editor-settings/symbolGroups';
import { useCodeHistoryStore } from '../../state/codeHistoryStore';
import { useEditorPreferencesStore } from '../../state/editor/editorPreferencesStore';
import { DEFAULT_RAIL_STYLE, DEFAULT_SYMBOL_BAR_SHOWN, useSymbolBarStore } from '../../state/editor/symbolBarStore';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import { useToolbarConfigStore } from '../../state/toolbarConfigStore';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';

// The editor settings menu on the editor route through the in-place panel (the twin of the native menu):
// every entry applies to the live page, toggles keep the menu open, Format Code confirms, Symbol Bar opens its sheet.

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
  format: vi.fn(async (p: { lang: string; code: string }) => `# formatted\n${p.code}`),
  loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })),
};

// reset before the screen mounts: a store write while a screen is still mounted is an update outside act
beforeEach(() => {
  useRunnerSettingStore.setState({ runner: 'backend' });
  useToolbarConfigStore.getState().resetToDefault();
  useEditorPreferencesStore.setState(DEFAULT_EDITOR_PREFERENCES);
  useSymbolBarStore.setState({ shown: DEFAULT_SYMBOL_BAR_SHOWN, groups: resetGroups(), railStyle: DEFAULT_RAIL_STYLE });
  useCodeHistoryStore.setState({ snapshots: [] });
  vi.mocked(gateway.format).mockClear();
});
afterEach(() => useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER }));

const eventually = (check: () => void | Promise<void>, timeout = 8_000) => waitFor(check, { timeout });

async function openEditor(): Promise<{ session: FlowSession; editor: FlowCodeEditor }> {
  setStackHeadersRendered(true);
  const ports: LeetCodePortOverrides = {
    gateway,
    voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort,
    audio: createFakeAudioRecorder(),
  };
  const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
  return { session, editor: await session.editor() };
}

async function openSettings(): Promise<void> {
  await userEvent.click(screen.getByTestId('EditorSettingsButton'));
  await waitFor(() => expect(screen.getByTestId('EditorSettingsButtonContent')).toBeTruthy());
}

const press = (testID: string) => userEvent.click(screen.getByTestId(testID));
const checked = (testID: string) => screen.getByTestId(testID).getAttribute('aria-checked');

async function raiseKeyboard(): Promise<void> {
  Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: true });
  window.dispatchEvent(new Event('mcp:keyboardframe'));
  await waitFor(() => expect(screen.queryByTestId('EditorSyntax-Paren')).not.toBeNull(), { timeout: 5_000 });
}

describe('the editor settings menu on the editor route', () => {
  it('given the editor route, when it opens, then the settings pill, Info and More show with the same ids and More has no Format row', async () => {
    await openEditor();

    expect(screen.getByTestId('ProblemEditorHeaderGroup0')).toBeTruthy();
    expect(screen.getByTestId('EditorSettingsButton')).toBeTruthy();
    expect(screen.getByTestId('ProblemEditorInfoButton')).toBeTruthy();
    await press('ProblemEditorMenuButton');
    await waitFor(() => expect(screen.getByTestId('ProblemEditorMenuButtonContent')).toBeTruthy());
    expect(screen.queryByTestId('ProblemEditorMenuFormat')).toBeNull();
    expect(screen.getByTestId('ProblemEditorMenuHistory')).toBeTruthy();
  });

  it('given the root menu, when Font size then 16px is pressed, then the menu returns to the root and the page reports 16 with line height 24', async () => {
    const { editor } = await openEditor();
    await openSettings();

    await press('EditorSettingsFontSize');
    await press('EditorSettingsFontSize-16');

    await eventually(async () => expect(await editor.config()).toMatchObject({ fontSize: 16, lineHeight: 24 }));
    expect(screen.getByTestId('EditorSettingsWrap')).toBeTruthy();
    expect(screen.queryByTestId('EditorSettingsFontSize-16')).toBeNull();
    await press('EditorSettingsFontSize');
    expect(checked('EditorSettingsFontSize-16')).toBe('true');
    expect(checked('EditorSettingsFontSize-14')).toBe('false');
  });

  it('given Line wrap on, when it is pressed, then the menu stays open, the check flips and the page wraps no more', async () => {
    const { editor } = await openEditor();
    await openSettings();
    expect(checked('EditorSettingsWrap')).toBe('true');

    await press('EditorSettingsWrap');

    expect(screen.getByTestId('EditorSettingsButtonContent')).toBeTruthy();
    await eventually(() => expect(checked('EditorSettingsWrap')).toBe('false'));
    await eventually(async () => expect((await editor.config()).wrap).toBe(false));
  });

  it('given text with spaces, when Show Whitespace is pressed, then the page draws whitespace marks', async () => {
    const { editor } = await openEditor();
    await editor.setText('a  b');
    await openSettings();

    await press('EditorSettingsWhitespace');

    await eventually(() => expect(editor.whitespaceMarks()).toBeGreaterThan(0));
    await eventually(() => expect(checked('EditorSettingsWhitespace')).toBe('true'));
  });

  it('given Tabs 2 picked, when Enter is pressed after a line that ends in a colon, then the page indents two spaces', async () => {
    const { editor } = await openEditor();
    await editor.setText('if x:');
    await editor.select(5);
    await openSettings();

    await press('EditorSettingsTabs');
    await press('EditorSettingsTabs-2');
    await eventually(async () => expect((await editor.config()).tabSize).toBe(2));
    await editor.press('Enter');

    await eventually(() => expect(editor.text()).toBe('if x:\n  '));
  });

  it('given every setting changed, when the language is switched to JavaScript, then the page keeps them', async () => {
    const { editor } = await openEditor();
    await openSettings();
    await press('EditorSettingsFontSize');
    await press('EditorSettingsFontSize-18');
    await press('EditorSettingsWhitespace');
    await eventually(async () => expect(await editor.config()).toMatchObject({ fontSize: 18, whitespace: true }));

    await userEvent.click(screen.getByTestId('ProblemEditorLanguagePicker-Trigger'));
    await waitFor(() => expect(screen.getByTestId('ProblemEditorLanguagePicker-JavaScript')).toBeTruthy());
    await userEvent.click(screen.getByTestId('ProblemEditorLanguagePicker-JavaScript'));

    await eventually(async () => expect(await editor.config()).toMatchObject({ language: 'javascript', fontSize: 18, lineHeight: 27, whitespace: true }));
  });

  it('given Format Code, when it is opened and Format is pressed, then the card explains, the gateway formats once and the buffer is snapshotted as format', async () => {
    await openEditor();
    await openSettings();

    await press('EditorSettingsFormat');
    expect(within(screen.getByTestId('EditorSettingsFormatCard')).getByText('Your code will be reformatted.')).toBeTruthy();
    expect(gateway.format).not.toHaveBeenCalled();
    await press('EditorSettingsFormatConfirm');

    await waitFor(() => expect(gateway.format).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(useCodeHistoryStore.getState().snapshots.map((snap) => snap.reason)).toContain('format'));
  });
});

describe('the Symbol Bar sheet on the editor route', () => {
  it('given the editor held the caret and the keyboard is up, when Symbol Bar is pressed, then the sheet opens, the rail is gone and the editor let go; and Done gives the caret back', async () => {
    const { session, editor } = await openEditor();
    await editor.focus();
    await raiseKeyboard();
    await openSettings();

    await press('EditorSettingsSymbolBar');

    await waitFor(() => expect(screen.getByTestId('SymbolBarSheetHeader')).toBeTruthy());
    await waitFor(() => expect(editor.focused()).toBe(false));
    await waitFor(() => expect(screen.queryByTestId('EditorSyntax-Paren')).toBeNull());

    await press('SymbolBarDone');

    await waitFor(() => expect(screen.queryByTestId('SymbolBarSheetHeader')).toBeNull());
    await waitFor(() => expect(editor.focused()).toBe(true));
    expect(session).toBeTruthy();
  });

  it('given the editor held no caret, when the sheet is opened and Done is pressed, then the editor stays without focus', async () => {
    const { editor } = await openEditor();
    await press('ProblemEditorTapAway');
    await waitFor(() => expect(editor.focused()).toBe(false));
    await openSettings();

    await press('EditorSettingsSymbolBar');
    await waitFor(() => expect(screen.getByTestId('SymbolBarSheetHeader')).toBeTruthy());
    await press('SymbolBarDone');

    await waitFor(() => expect(screen.queryByTestId('SymbolBarSheetHeader')).toBeNull());
    expect(editor.focused()).toBe(false);
  });

  it('given the symbols style and the keyboard up, when a group is reordered in the sheet and Done is pressed, then the rail shows the new order', async () => {
    useSymbolBarStore.setState({ railStyle: 'symbols' });
    const { editor } = await openEditor();
    await editor.focus();
    Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: true });
    window.dispatchEvent(new Event('mcp:keyboardframe'));
    await waitFor(() => expect(screen.queryByTestId('SymbolBarTab')).not.toBeNull(), { timeout: 5_000 });
    await openSettings();
    await press('EditorSettingsSymbolBar');
    await waitFor(() => expect(screen.getByTestId('SymbolBarSheetHeader')).toBeTruthy());

    await act(async () => useSymbolBarStore.getState().move(1, 2));
    await press('SymbolBarDone');

    await waitFor(() => expect(screen.queryByTestId('SymbolBarGroup-g3')).not.toBeNull());
    const ids = screen.queryAllByTestId(/^SymbolBarGroup-/).map((group) => group.getAttribute('data-testid')?.replace('SymbolBarGroup-', ''));
    expect(ids.slice(0, 3)).toEqual(['g1', 'g3', 'g2']);
  });
});
