import { createFakeAudioRecorder, createFakeVoiceEditTransport, act, fireEvent, screen, userEvent, waitFor, type FlowCodeEditor, type FlowSession } from '@lib/flow-test-harness';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../tests/flow/loggedLines';
import { depthX, GUTTER, gapY, ROW_HEIGHT, rowTop } from '../../domain/blocks/blockLayout';
import { DEFAULT_BLOCK_VIEW, useBlockViewSettingStore } from '../../state/blockViewSettingStore';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import { createFakeCodeRunner, recordingEditorBridge, type RecordingEditorBridge } from '../../testing';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';

// Dragging a block by its grip on the real CodeMirror page: the pointer events of the test go through the real gesture handler stub, the planner and the typed API, and the
// recording bridge shows the one call a drop made. Every input is computed from the layout the view and the planner share. Not provable here: the feel of a finger, a drag winning
// against the page's scroll on a phone, and the ghost's motion (the stubs of reanimated and of the gesture handler cannot show it).

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
  useBlockViewSettingStore.setState({ blockView: DEFAULT_BLOCK_VIEW });
});
afterEach(() => useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER }));

const DEF_IF = 'def f(x):\n    if x:\n        a = 1\n        b = 2\n    return b\n';
const MOVED = 'def f(x):\n    if x:\n        a = 1\n    b = 2\n    return b\n';
const ROWS = ['def f(x):, level 1', 'if x:, level 2', 'a = 1, level 3', 'b = 2, level 3', 'return b, level 2'];
const ROWS_MOVED = ['def f(x):, level 1', 'if x:, level 2', 'a = 1, level 3', 'b = 2, level 2', 'return b, level 2'];
const rowIndex = { def: 0, if: 1, a: 2, b: 3, return: 4 } as const;

async function openWith(code: string, options: { recording?: RecordingEditorBridge; ports?: Partial<LeetCodePortOverrides> } = {}): Promise<{ session: FlowSession; editor: FlowCodeEditor; recording: RecordingEditorBridge }> {
  useBlockViewSettingStore.setState({ blockView: true });
  const recording = options.recording ?? recordingEditorBridge();
  const ports: LeetCodePortOverrides = { gateway, voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort, audio: createFakeAudioRecorder(), editorBridge: recording.bridge, ...options.ports };
  const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
  const editor = await session.editor();
  await editor.setText(code);
  return { session, editor, recording };
}

const eventually = (check: () => void | Promise<void>, timeout = 8_000) => waitFor(check, { timeout });
const rows = () => screen.queryAllByTestId(/^BlockRow-\d+$/).map((row) => row.getAttribute('aria-label'));
const drops = (recording: RecordingEditorBridge) => recording.transacts.filter((call) => call.options?.label === 'blocks.move' || call.options?.label === 'blocks.insert');
const readLines = async () => (await loggedLines('leetcode.blocks.read')).length;

const handleBox = (index: number) => ({ x: 0, y: rowTop(index), width: GUTTER, height: ROW_HEIGHT });
const GRAB = { x: GUTTER / 2, y: ROW_HEIGHT / 2 };
/** where the finger is, in the grip's own coordinates, when it is at the slot of (gap, depth) of the list */
const at = (index: number, gap: number, depth: number) => ({ x: depthX(depth), y: gapY(gap) - rowTop(index) });
const pagePoint = (index: number, point: { x: number; y: number }) => ({ clientX: handleBox(index).x + point.x, clientY: handleBox(index).y + point.y });

async function dragTo(session: FlowSession, index: number, gap: number, depth: number): Promise<void> {
  await session.layout(`BlockHandle-${index}`, handleBox(index));
  await session.stroke(`BlockHandle-${index}`, [GRAB, at(index, gap, depth)], { units: 'px' });
}

/** the grip held down and moved to a slot, the finger not lifted */
async function holdAt(session: FlowSession, index: number, gap: number, depth: number): Promise<HTMLElement> {
  await session.layout(`BlockHandle-${index}`, handleBox(index));
  const handle = screen.getByTestId(`BlockHandle-${index}`);
  act(() => {
    fireEvent.pointerDown(handle, pagePoint(index, GRAB));
    fireEvent.pointerMove(handle, pagePoint(index, at(index, gap, depth)));
  });
  return handle;
}

const release = (handle: HTMLElement, index: number, gap: number, depth: number) => act(() => void fireEvent.pointerUp(handle, pagePoint(index, at(index, gap, depth))));

async function drawn(code: string, options?: Parameters<typeof openWith>[1]) {
  const opened = await openWith(code, options);
  await eventually(() => expect(rows()).toHaveLength(code.trimEnd().split('\n').length));
  return opened;
}

describe('dragging a block by its grip', () => {
  it('given b = 2 in the if, when its grip is dragged to the boundary after the if at depth 1, then one moveNode is sent at the revision of the rows, the text moves, the rows redraw and one undo restores it', async () => {
    const { session, editor, recording } = await drawn(DEF_IF);
    const tree = await editor.tree();
    const ifRange = { from: DEF_IF.indexOf('if x:'), to: DEF_IF.indexOf('b = 2') + 'b = 2'.length };

    await dragTo(session, rowIndex.b, 4, 1);

    await eventually(() => expect(drops(recording)).toHaveLength(1));
    const [call] = drops(recording);
    expect(call?.steps).toEqual([{ op: 'moveNode', node: { from: DEF_IF.indexOf('b = 2'), to: DEF_IF.indexOf('b = 2') + 5 }, to: { after: ifRange }, expect: 'b = 2' }]);
    expect(call?.options).toEqual({ at: tree.revision, instance: tree.instance, label: 'blocks.move', budgetMs: 1000 });
    expect(call?.reply.ok).toBe(true);
    await eventually(() => expect(editor.text()).toBe(MOVED));
    await eventually(() => expect(rows()).toEqual(ROWS_MOVED));
    expect(await editor.undo()).toBe(true);
    expect(editor.text()).toBe(DEF_IF);
    await eventually(() => expect(rows()).toEqual(ROWS));
  });

  it('given b = 2 in the if, when its grip is dragged to the boundary just above itself, then no call is made and the text is unchanged', async () => {
    const { session, editor, recording } = await drawn(DEF_IF);
    await forgetLoggedLines();

    await dragTo(session, rowIndex.b, 3, 2);

    await eventually(async () => expect((await loggedLines('leetcode.blocks.drag_end')).length).toBe(1));
    expect(drops(recording)).toHaveLength(0);
    expect(editor.text()).toBe(DEF_IF);
  });

  it('given a drag that has not ended, when the finger is over a slot, then the drop line is on show and says where it would land, and it is gone when the finger lifts', async () => {
    const { session, editor, recording } = await drawn(DEF_IF);

    const handle = await holdAt(session, rowIndex.b, 4, 1);

    await eventually(() => expect(screen.getByTestId('BlockDropIndicatorText').textContent).toBe('Move after if x:'));
    expect(screen.getByTestId('BlockGhost').textContent).toBe('b = 2');
    expect(editor.text()).toBe(DEF_IF);
    await session.screenshot('block-view-drag');
    release(handle, rowIndex.b, 4, 1);
    await eventually(() => expect(screen.queryByTestId('BlockDropIndicator')).toBeNull());
    expect(screen.queryByTestId('BlockGhost')).toBeNull();
    await eventually(() => expect(drops(recording)).toHaveLength(1));
    await eventually(() => expect(rows()).toEqual(ROWS_MOVED));
  });

  it('given a drag that has not ended, when the text changes under it, then the rows stay as they were until the finger lifts and then follow the text', async () => {
    const { session, editor } = await drawn(DEF_IF);
    const handle = await holdAt(session, rowIndex.b, 3, 2);
    await forgetLoggedLines();

    await editor.setText(`${DEF_IF}c = 3\n`);
    await eventually(async () => expect(await readLines()).toBeGreaterThan(0));

    expect(rows()).toHaveLength(5);
    release(handle, rowIndex.b, 3, 2);
    await eventually(() => expect(rows()).toHaveLength(6));
  });

  it('given the text changes after the rows were drawn and before the tree is read again, when the old block is dropped, then the call is answered stale, the text holds only the change, and the rows follow the text once the read goes through', async () => {
    const { session, editor, recording } = await drawn(DEF_IF);
    const openTrees = recording.holdTrees();
    const typed = `${DEF_IF}c = 3\n`;
    await editor.setText(typed);

    await dragTo(session, rowIndex.b, 4, 1);

    await eventually(() => expect(drops(recording)).toHaveLength(1));
    const reply = drops(recording)[0]?.reply;
    expect(reply && !reply.ok && reply.error.code).toBe('stale');
    expect(editor.text()).toBe(typed);
    expect(screen.getByTestId('BlockViewMessage').textContent).toMatch(/code changed/);
    expect(rows()).toHaveLength(5);
    openTrees();
    await eventually(() => expect(rows()).toHaveLength(6));
  });

  it('given the engine refuses the move, when the grip is dropped, then the text is unchanged, the message says why and the rows stay', async () => {
    const recording = recordingEditorBridge({ answer: (steps) => (steps.some((step) => step.op === 'moveNode') ? { ok: false, revision: 0, error: { code: 'would-not-parse', message: 'no', errors: [], reason: 'syntax' } } : null) });
    const { session, editor } = await drawn(DEF_IF, { recording });

    // the double answers at once, so the drop ends inside this call and its state updates are part of it
    await act(async () => {
      await dragTo(session, rowIndex.b, 4, 1);
    });

    await eventually(() => expect(screen.getByTestId('BlockViewMessage').textContent).toBe('That would break the code, so nothing changed.'));
    expect(editor.text()).toBe(DEF_IF);
    expect(rows()).toHaveLength(5);
  });

  it('given a half-typed line, when a block is dropped on the boundary after it, then no call is made because the model withheld that slot', async () => {
    const code = 'def f(xs):\n    a = 1\n    for x in\n    return a\n';
    const { session, editor, recording } = await drawn(code);
    const labels = rows();
    const typed = labels.indexOf('for x in, plain text');
    const grip = labels.indexOf('a = 1, level 2');
    expect(typed).toBeGreaterThan(0);
    expect(grip).toBeGreaterThan(0);
    await forgetLoggedLines();

    await dragTo(session, grip, typed + 1, 1);

    await eventually(async () => expect((await loggedLines('leetcode.blocks.drag_end')).length).toBe(1));
    expect(drops(recording)).toHaveLength(0);
    expect(editor.text()).toBe(code);
  });

  it('given a grip that is pressed and lifted without moving, when the finger lifts, then no call is made', async () => {
    const { session, recording } = await drawn(DEF_IF);
    await session.layout(`BlockHandle-${rowIndex.b}`, handleBox(rowIndex.b));
    const handle = screen.getByTestId(`BlockHandle-${rowIndex.b}`);

    act(() => {
      fireEvent.pointerDown(handle, pagePoint(rowIndex.b, GRAB));
      fireEvent.pointerUp(handle, pagePoint(rowIndex.b, GRAB));
    });

    expect(screen.queryByTestId('BlockDropIndicator')).toBeNull();
    expect(drops(recording)).toHaveLength(0);
  });

  it('given a result card on show, when a block is dropped, then the card is cleared like after any change to the text', async () => {
    const runner = createFakeCodeRunner();
    const { session } = await drawn(DEF_IF, { ports: { runner } });
    await userEvent.click(screen.getByTestId('ProblemEditorRunLocalButton'));
    await eventually(() => expect(screen.getByTestId('ProblemEditorRunOutcomeStatus')).toBeTruthy());

    await dragTo(session, rowIndex.b, 4, 1);

    await eventually(() => expect(screen.queryByTestId('ProblemEditorRunOutcomeStatus')).toBeNull());
    await eventually(() => expect(rows()).toEqual(ROWS_MOVED));
  });
});
