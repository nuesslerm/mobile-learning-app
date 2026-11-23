import {
  act,
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  fireEvent,
  screen,
  waitFor,
  type FlowCodeEditor,
  type FlowSession,
} from '@lib/flow-test-harness';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { DEFAULT_TUNING } from '../../domain/editor/cursorDrag';
import { stepChars, stepLines, stepWords } from '../../domain/editor/cursorMotion';
import { DEFAULT_CURSOR_HANDLE, useCursorHandleSettingStore } from '../../state/editor/cursorHandleSettingStore';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import { useSymbolBarStore } from '../../state/editor/symbolBarStore';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';

// The cursor handle on the real editor, through the screen: a drag on the pad moves the caret by what the finger covered, a selection keeps its
// anchor, React hears the selection once at the end, a tap only focuses, and a caret move is no undo step. Not provable here: the feel, the
// keyboard staying up on Android and iOS, and RNGH against the workspace's tab swipe on a native build.

const { CHAR_PX, CHAR_ZONE, WORD_PX, LINE_PX } = DEFAULT_TUNING;

const ACCEPTED: RunResponse = { source: 'backend-run', kind: 'test', state: 'SUCCESS', requestId: 'flow-request', run_success: true, status_msg: 'Accepted', total_correct: 5, total_testcases: 5 };
const gateway: LeetCodeGatewayPort = {
  userStatus: vi.fn(async () => ({ isSignedIn: true, username: 'flow-user' })),
  test: vi.fn(async () => ({ ...ACCEPTED, interpret_id: 'flow-interpret' })),
  submit: vi.fn(async () => ACCEPTED),
  poll: vi.fn(async () => ACCEPTED),
  runLocal: vi.fn(async () => ACCEPTED),
  format: vi.fn(async (p: { lang: string; code: string }) => p.code),
  loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })),
};

// set before the screen mounts: a store write while a screen is still mounted is an update outside act
beforeEach(() => {
  useRunnerSettingStore.setState({ runner: 'backend' });
  useSymbolBarStore.setState({ railStyle: 'classic' });
  useCursorHandleSettingStore.setState({ cursorHandle: DEFAULT_CURSOR_HANDLE });
});
afterEach(() => useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER }));

const eventually = (check: () => void | Promise<void>, timeout = 8_000) => waitFor(check, { timeout });

async function openEditor(handle: boolean): Promise<{ session: FlowSession; editor: FlowCodeEditor }> {
  useCursorHandleSettingStore.setState({ cursorHandle: handle });
  const ports: LeetCodePortOverrides = { flags: { cursorPad: true }, gateway, voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort, audio: createFakeAudioRecorder() };
  const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
  return { session, editor: await session.editor() };
}

async function raiseKeyboard(): Promise<void> {
  Object.assign(window, { __MCP_KEYBOARD_VISIBLE__: true });
  window.dispatchEvent(new Event('mcp:keyboardframe'));
  await waitFor(() => expect(screen.queryByTestId('EditorSyntax-Paren')).not.toBeNull(), { timeout: 5_000 });
}

// the pad has a box, so the gesture stub reads the finger's position against it
async function withHandle(session: FlowSession): Promise<void> {
  await session.layout('EditorCursorHandle', { x: 0, y: 700, width: 56, height: 50 });
}

const WORDS = 'alpha beta.gamma(delta, epsilon) zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau';
const LINES = 'first line of code\nsecond\nthird line is rather long\nfourth\nfifth line';

const dragRight = (session: FlowSession, px: number) => session.drag('EditorCursorHandle', { x: 10, y: 25 }, { x: 10 + px, y: 25 }, { units: 'px' });
const dragDown = (session: FlowSession, px: number) => session.drag('EditorCursorHandle', { x: 25, y: 10 }, { x: 25, y: 10 + px }, { units: 'px' });
const copyEnabled = () => getComputedStyle(screen.getByTestId('EditorAction-Copy')).opacity === '1';

describe('the cursor handle', () => {
  it('given the setting is off, when the keyboard is up, then the action row has no cursor handle', async () => {
    await openEditor(false);
    await raiseKeyboard();

    expect(screen.queryByTestId('EditorCursorHandle')).toBeNull();
    expect(screen.queryByTestId('EditorActionToolbarLeading')).toBeNull();
    expect(screen.getByTestId('EditorActionToolbar')).toBeTruthy();
  });

  it('given the setting is on, when the keyboard is up, then the handle sits in the row’s leading slot, outside the region that scrolls', async () => {
    await openEditor(true);
    await raiseKeyboard();

    expect(screen.getByTestId('EditorActionToolbarLeading').contains(screen.getByTestId('EditorCursorHandle'))).toBe(true);
    expect(screen.getByTestId('EditorActionToolbar').contains(screen.getByTestId('EditorCursorHandle'))).toBe(false);
    expect(screen.getByTestId('EditorActionToolbar').contains(screen.getByTestId('EditorAction-Done'))).toBe(true);
  });

  it('given the handle on, when a screen reader reaches it, then it is an adjustable control with a name, a hint and the caret’s line and column as its value, and the keys keep their button role', async () => {
    const { editor } = await openEditor(true);
    await raiseKeyboard();
    await editor.setText('abc\ndef');
    await editor.select(5);

    const pad = screen.getByTestId('EditorCursorHandle');

    expect(pad.getAttribute('role')).toBe('slider');
    expect(pad.getAttribute('aria-label')).toBe('Cursor handle');
    await eventually(() => expect(pad.getAttribute('aria-valuetext')).toBe('Line 2, column 2'));
    await editor.select(1, 6);
    await eventually(() => expect(pad.getAttribute('aria-valuetext')).toBe('Selected from line 1, column 2 to line 2, column 3'));
    for (const [testID, name] of [['EditorAction-Undo', 'Undo'], ['EditorAction-Copy', 'Copy selected code'], ['EditorAction-Done', 'Done editing']]) {
      const key = screen.getByTestId(testID);
      expect(key.getAttribute('role')).toBe('button');
      expect(key.getAttribute('aria-label')).toBe(name);
    }
  });

  it('given a caret at 0, when the handle is dragged three characters to the right, then the caret is at 3, and farther it lands on a word start', async () => {
    const { session, editor } = await openEditor(true);
    await raiseKeyboard();
    await withHandle(session);
    await editor.setText(WORDS);
    await editor.select(0);

    await dragRight(session, 3 * CHAR_PX);
    await eventually(() => expect(editor.selection()).toEqual({ start: 3, end: 3 }));

    await editor.select(0);
    await dragRight(session, CHAR_ZONE * CHAR_PX + 2 * WORD_PX);
    const head = stepWords(WORDS, stepChars(WORDS, 0, CHAR_ZONE), 2);
    await eventually(() => expect(editor.selection()).toEqual({ start: head, end: head }));
    expect(stepWords(WORDS, stepWords(WORDS, head, -1), 1)).toBe(head);
  });

  it('given a caret in the middle of a line, when the handle is dragged down, then whole lines go by and the column stays', async () => {
    const { session, editor } = await openEditor(true);
    await raiseKeyboard();
    await withHandle(session);
    await editor.setText(LINES);
    await editor.select(8);

    await dragDown(session, 2 * LINE_PX);
    const head = stepLines(LINES, 8, 2);
    await eventually(() => expect(editor.selection()).toEqual({ start: head, end: head }));

    await editor.select(8);
    await dragDown(session, -LINE_PX);
    await eventually(() => expect(editor.selection()).toEqual({ start: 0, end: 0 }));
  });

  it('given a selection of 2 to 5, when the handle is dragged right, then the start stays and the end grows', async () => {
    const { session, editor } = await openEditor(true);
    await raiseKeyboard();
    await withHandle(session);
    await editor.setText(WORDS);
    await editor.select(2, 5);

    await dragRight(session, 3 * CHAR_PX);

    await eventually(() => expect(editor.selection()).toEqual({ start: 2, end: 8 }));
    await eventually(() => expect(copyEnabled()).toBe(true));
  });

  it('given a selection, when the head is held away from React during the drag and the finger lifts, then React hears the selection once at the end', async () => {
    const { session, editor } = await openEditor(true);
    await raiseKeyboard();
    await withHandle(session);
    await editor.setText(WORDS);
    await editor.select(2, 5);
    await eventually(() => expect(copyEnabled()).toBe(true));
    const pad = screen.getByTestId('EditorCursorHandle');
    const at = (x: number) => ({ clientX: x, clientY: 725 });

    act(() => {
      fireEvent.pointerDown(pad, at(10));
      for (let step = 1; step <= 3; step += 1) fireEvent.pointerMove(pad, at(10 - step * CHAR_PX));
    });
    // the page has collapsed the selection onto the anchor; the screen has not been told, so Copy is still on
    await eventually(() => expect(editor.selection()).toEqual({ start: 2, end: 2 }));
    expect(copyEnabled()).toBe(true);

    act(() => {
      fireEvent.pointerUp(pad, at(10 - 3 * CHAR_PX));
    });

    await eventually(() => expect(copyEnabled()).toBe(false));
  });

  it('given a tap that does not move, when the handle is pressed with the editor out of focus, then the caret is unchanged and the editor has the focus', async () => {
    const { session, editor } = await openEditor(true);
    await raiseKeyboard();
    await withHandle(session);
    await editor.setText(WORDS);
    await editor.select(4);
    const frame = document.querySelector('iframe');
    (frame?.contentDocument?.querySelector('.cm-content') as HTMLElement | null)?.blur();
    await eventually(() => expect(editor.focused()).toBe(false));

    await session.tap('EditorCursorHandle');

    await eventually(() => expect(editor.focused()).toBe(true));
    expect(editor.selection()).toEqual({ start: 4, end: 4 });
  });

  it('given a drag followed by one typed character and the toolbar Undo, when Undo is pressed, then the typed character goes and the text is the original', async () => {
    const { session, editor } = await openEditor(true);
    await raiseKeyboard();
    await withHandle(session);
    await editor.setText(WORDS);
    await editor.select(0);

    await dragRight(session, 3 * CHAR_PX);
    await eventually(() => expect(editor.selection()).toEqual({ start: 3, end: 3 }));
    await editor.type('X');
    await eventually(() => expect(editor.text()).toBe(`${WORDS.slice(0, 3)}X${WORDS.slice(3)}`));
    await session.tap('EditorAction-Undo');

    await eventually(() => expect(editor.text()).toBe(WORDS));
  });
});
