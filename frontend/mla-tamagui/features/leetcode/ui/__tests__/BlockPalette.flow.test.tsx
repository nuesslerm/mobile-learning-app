import { createFakeAudioRecorder, createFakeVoiceEditTransport, screen, waitFor, type FlowCodeEditor, type FlowSession } from '@lib/flow-test-harness';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { forgetLoggedLines, loggedLines } from '../../../../tests/flow/loggedLines';
import { CHIP_WIDTH, chipOrigin, depthX, gapY, PALETTE_HEIGHT } from '../../domain/blocks/blockLayout';
import { PYTHON_PALETTE, type PaletteItem } from '../../domain/blocks/palette';
import { DEFAULT_BLOCK_VIEW, useBlockViewSettingStore } from '../../state/blockViewSettingStore';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import { recordingEditorBridge, type RecordingEditorBridge } from '../../testing';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';

// The palette on the real CodeMirror page: a chip is dragged by the real gesture handler stub to a slot and one insertSnippet lands there. Inputs are computed from the layout the view and the planner share.
// Not provable here: a finger on a chip, and the ghost's motion.

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
const STARTER = 'class Solution:\n    def twoSum(self, nums: List[int], target: int) -> List[int]:\n        ';

async function open(code: string, recording: RecordingEditorBridge = recordingEditorBridge()): Promise<{ session: FlowSession; editor: FlowCodeEditor; recording: RecordingEditorBridge }> {
  useBlockViewSettingStore.setState({ blockView: true });
  const ports: LeetCodePortOverrides = { gateway, voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort, audio: createFakeAudioRecorder(), editorBridge: recording.bridge };
  const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
  const editor = await session.editor();
  await editor.setText(code);
  return { session, editor, recording };
}

const eventually = (check: () => void | Promise<void>, timeout = 8_000) => waitFor(check, { timeout });
const rows = () => screen.queryAllByTestId(/^BlockRow-\d+$/).map((row) => row.getAttribute('aria-label'));
const inserts = (recording: RecordingEditorBridge) => recording.transacts.filter((call) => call.options?.label === 'blocks.insert');
const dragEnds = async () => (await loggedLines('leetcode.blocks.drag_end')).length;

const GRAB = { x: CHIP_WIDTH / 2, y: PALETTE_HEIGHT / 2 };
const indexOfChip = (item: PaletteItem) => PYTHON_PALETTE.indexOf(item);
const chipBox = (item: PaletteItem) => ({ ...chipOrigin(indexOfChip(item)), width: CHIP_WIDTH, height: PALETTE_HEIGHT });

/** a drag of the chip to the slot at (gap, depth): the end is where the finger is, in the chip's own coordinates */
async function dragChip(session: FlowSession, item: PaletteItem, gap: number, depth: number): Promise<void> {
  const origin = chipOrigin(indexOfChip(item));
  await session.layout(`BlockPaletteItem-${item.id}`, chipBox(item));
  await session.stroke(`BlockPaletteItem-${item.id}`, [GRAB, { x: depthX(depth) - origin.x, y: gapY(gap) - origin.y }], { units: 'px' });
}

const indented = (snippet: string, prefix: string) => snippet.split('\n').map((line) => prefix + line);

describe('the palette above the rows', () => {
  it('given the block view on, when the page has read the code, then the five chips are there in order', async () => {
    await open(DEF_IF);

    await eventually(() => expect(rows()).toHaveLength(5));

    expect(PYTHON_PALETTE.map((item) => screen.getByTestId(`BlockPaletteItem-${item.id}`).getAttribute('aria-label'))).toEqual(['Add if', 'Add for', 'Add while', 'Add def', 'Add return']);
  });

  it.each(PYTHON_PALETTE.map((item) => [item.id, item] as const))('given the %s chip, when it is dragged to the boundary after a = 1 at depth 2, then one insertSnippet lands at the if body\'s indent and one undo takes it away', async (_id, item) => {
    const { session, editor, recording } = await open(DEF_IF);
    await eventually(() => expect(rows()).toHaveLength(5));
    const tree = await editor.tree();
    const gap = 3;

    await dragChip(session, item, gap, 2);

    await eventually(() => expect(inserts(recording)).toHaveLength(1));
    const [call] = inserts(recording);
    expect(call?.steps).toEqual([{ op: 'insertSnippet', at: { after: { from: DEF_IF.indexOf('a = 1'), to: DEF_IF.indexOf('a = 1') + 5 } }, text: item.snippet }]);
    expect(call?.options).toEqual({ at: tree.revision, instance: tree.instance, label: 'blocks.insert', budgetMs: 1000 });
    const expected = ['def f(x):', '    if x:', '        a = 1', ...indented(item.snippet, '        '), '        b = 2', '    return b', ''].join('\n');
    await eventually(() => expect(editor.text()).toBe(expected));
    await eventually(() => expect(rows()).toHaveLength(5 + item.snippet.split('\n').length));
    expect(await editor.undo()).toBe(true);
    expect(editor.text()).toBe(DEF_IF);
    await eventually(() => expect(rows()).toHaveLength(5));
  });

  it('given a chip, when it is dragged to the start of the if body, then the destination is the start of the if\'s first block', async () => {
    const { session, recording, editor } = await open(DEF_IF);
    await eventually(() => expect(rows()).toHaveLength(5));
    const item = PYTHON_PALETTE[0] as PaletteItem;

    await dragChip(session, item, 2, 2);

    await eventually(() => expect(inserts(recording)).toHaveLength(1));
    const ifRange = { from: DEF_IF.indexOf('if x:'), to: DEF_IF.indexOf('b = 2') + 'b = 2'.length };
    expect(inserts(recording)[0]?.steps).toEqual([{ op: 'insertSnippet', at: { into: ifRange, at: 'start', block: 0 }, text: item.snippet }]);
    await eventually(() => expect(editor.text()).toBe(['def f(x):', '    if x:', ...indented(item.snippet, '        '), '        a = 1', '        b = 2', '    return b', ''].join('\n')));
    await eventually(() => expect(rows()).toHaveLength(7));
  });

  it('given a half-typed line, when a chip is dropped on the boundary after it, then no call is made because the model withheld that slot', async () => {
    const code = 'def f(xs):\n    a = 1\n    for x in\n    return a\n';
    const { session, editor, recording } = await open(code);
    await eventually(() => expect(rows()).toHaveLength(4));
    const typed = rows().indexOf('for x in, plain text');
    expect(typed).toBeGreaterThan(0);
    await forgetLoggedLines();

    await dragChip(session, PYTHON_PALETTE[1] as PaletteItem, typed + 1, 1);

    await eventually(async () => expect(await dragEnds()).toBe(1));
    expect(inserts(recording)).toHaveLength(0);
    expect(editor.text()).toBe(code);
  });

  it('given the text changes after the rows were drawn, when a chip is dropped, then the call is answered stale, nothing is inserted and the rows follow the text once the read goes through', async () => {
    const { session, editor, recording } = await open(DEF_IF);
    await eventually(() => expect(rows()).toHaveLength(5));
    const openTrees = recording.holdTrees();
    const typed = `${DEF_IF}c = 3\n`;
    await editor.setText(typed);

    await dragChip(session, PYTHON_PALETTE[0] as PaletteItem, 3, 2);

    await eventually(() => expect(inserts(recording)).toHaveLength(1));
    const reply = inserts(recording)[0]?.reply;
    expect(reply && !reply.ok && reply.error.code).toBe('stale');
    expect(editor.text()).toBe(typed);
    openTrees();
    await eventually(() => expect(rows()).toHaveLength(6));
  });

  it('given the untouched starter whose body is empty, when a chip is dropped into the def, then no call is made: an empty Python body is plain text', async () => {
    const { session, editor, recording } = await open(STARTER);
    await eventually(() => expect(rows().length).toBeGreaterThan(0));
    await forgetLoggedLines();

    await dragChip(session, PYTHON_PALETTE[0] as PaletteItem, rows().length, 2);

    await eventually(async () => expect(await dragEnds()).toBe(1));
    expect(inserts(recording)).toHaveLength(0);
    expect(editor.text()).toBe(STARTER);
  });
});
