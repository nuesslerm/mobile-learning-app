import { outline, type TreeValue } from '@lib/code-editor-api';
import { parseFor } from '@lib/code-editor-api/testing';
import { createFakeAudioRecorder, createFakeVoiceEditTransport, screen, userEvent, waitFor, type FlowCodeEditor, type FlowSession } from '@lib/flow-test-harness';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { buildBlockModel } from '../../domain/blocks/blockModel';
import { DEFAULT_BLOCK_VIEW, useBlockViewSettingStore } from '../../state/blockViewSettingStore';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';
import { rowDescription } from '../blocks/BlockRow';

// The block view on the real CodeMirror page: its rows come from the page's own Lezer tree, read through the typed API, and redraw as the text changes.
// Not provable here: how it looks on a phone (the golden is the browser's), and a finger on it.

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

async function openEditor(blockView: boolean, tab: 'code' | 'blocks' = 'blocks'): Promise<{ session: FlowSession; editor: FlowCodeEditor }> {
  useBlockViewSettingStore.setState({ blockView });
  const ports: LeetCodePortOverrides = { gateway, voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort, audio: createFakeAudioRecorder() };
  const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
  const editor = await session.editor();
  if (blockView && tab === 'blocks') await userEvent.click(screen.getByTestId('ProblemWorkspaceTab-blocks'));
  return { session, editor };
}

const drawn = () => screen.queryAllByTestId(/^BlockRow-\d+$/).map((row) => row.getAttribute('aria-label'));

/** The rows the unit tests' parser gives for a text: the model the real page's rows are compared with. */
function expectedRows(text: string): string[] {
  const tree: TreeValue = { instance: 'x', revision: 0, language: 'python', length: text.length, parsedTo: text.length, complete: true, detail: 'outline', root: outline('python', parseFor(text, 'python')) };
  return buildBlockModel({ text, tree }).rows.map(rowDescription);
}

const eventually = (check: () => void | Promise<void>, timeout = 8_000) => waitFor(check, { timeout });

describe('the block view under the editor', () => {
  it('given the setting is off, when the editor holds Python, then there is no block view', async () => {
    const { editor } = await openEditor(false);
    await editor.setText(DEF_IF);

    expect(screen.queryByTestId('ProblemEditorBlockView')).toBeNull();
    expect(screen.queryByTestId('BlockRow-0')).toBeNull();
  });

  it('given the setting is on and Python code in the editor, when the page has read it, then each statement is a row with its level', async () => {
    const { session, editor } = await openEditor(true);
    await editor.setText(DEF_IF);

    await eventually(() => expect(drawn()).toEqual(['def f(x):, level 1', 'if x:, level 2', 'a = 1, level 3', 'b = 2, level 3', 'return b, level 2']));

    expect(screen.getByTestId('ProblemEditorBlockView')).toBeTruthy();
    expect(screen.queryByTestId('BlockViewMessage')).toBeNull();
    await session.screenshot('block-view-rows');
  });

  it('given rows on show, when the editor is given another program, then the rows redraw to it', async () => {
    const { editor } = await openEditor(true);
    await editor.setText(DEF_IF);
    await eventually(() => expect(drawn()).toHaveLength(5));

    await editor.setText('for i in range(3):\n    print(i)\nx = 1\n');

    await eventually(() => expect(drawn()).toEqual(['for i in range(3):, level 1', 'print(i), level 2', 'x = 1, level 1']));
  });

  it('given a line typed half way, when the page has read the code, then that row is plain text and the statements around it keep their rows', async () => {
    const { editor } = await openEditor(true);
    const whole = 'def f(xs):\n    a = 1\n    return a\n';
    await editor.setText(whole);
    await eventually(() => expect(drawn()).toEqual(['def f(xs):, level 1', 'a = 1, level 2', 'return a, level 2']));

    await editor.setText('def f(xs):\n    a = 1\n    for x in\n    return a\n');

    await eventually(() => expect(drawn().some((row) => row?.endsWith('plain text'))).toBe(true));
    const rows = drawn();
    expect(rows).toContain('for x in, plain text');
    expect(rows).toContain('a = 1, level 2');
    expect(rows).toContain('return a, level 2');
  });

  it('given rows on show, when the picker is switched to JavaScript, then the block view is gone', async () => {
    const { editor } = await openEditor(true);
    await editor.setText(DEF_IF);
    await eventually(() => expect(drawn()).toHaveLength(5));

    await userEvent.click(screen.getByTestId('ProblemWorkspaceTab-code'));
    await userEvent.click(screen.getByTestId('ProblemEditorLanguagePicker-Trigger'));
    await userEvent.click(await screen.findByTestId('ProblemEditorLanguagePicker-JavaScript'));
    await userEvent.click(screen.getByTestId('ProblemWorkspaceTab-blocks'));

    await eventually(() => expect(screen.getByTestId('BlockViewUnavailable')).toBeTruthy());
    expect(screen.queryByTestId('ProblemEditorBlockView')).toBeNull();
  });

  it.each([
    ['a def with an if inside', DEF_IF],
    ['an if with an else', 'def f(x):\n    if x:\n        a = 1\n    else:\n        b = 2\n    return a\n'],
    ['a class with a method', 'class Solution:\n    def twoSum(self, nums, target):\n        seen = {}\n        return seen\n'],
    ['loops inside a function', 'def f(n):\n    for i in range(n):\n        while i:\n            i -= 1\n    return n\n'],
  ])('given %s, when the page has read it, then its rows are the rows the unit tests\' parser gives for the same text', async (_name, text) => {
    const { editor } = await openEditor(true);
    await editor.setText(text);

    await eventually(() => expect(drawn()).toEqual(expectedRows(text)));
  });

  it('given a decorated function, when the page has read it, then it is one row, and a one-line if is a row with its body line as plain text', async () => {
    const { editor } = await openEditor(true);
    await editor.setText('@cache\ndef f():\n    return 1\n');
    await eventually(() => expect(drawn()).toEqual(['@cache, level 1']));

    await editor.setText('if x: y\nz = 1\n');

    await eventually(() => expect(drawn()).toEqual(['if x: y, level 1', 'y, plain text', 'z = 1, level 1']));
  });

  it('given code that is only comments, when the page has read it, then the view says there are no statements', async () => {
    const { editor } = await openEditor(true);

    await editor.setText('# nothing yet\n');

    await eventually(() => expect(screen.getByTestId('BlockViewMessage').textContent).toBe('No statements yet. Type one in the editor.'));
    expect(drawn()).toEqual([]);
  });
});
