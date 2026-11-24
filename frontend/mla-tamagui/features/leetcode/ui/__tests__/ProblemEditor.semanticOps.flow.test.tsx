import {
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  screen,
  userEvent,
  waitFor,
  type FlowCodeEditor,
} from '@lib/flow-test-harness';
import type { Range, Step } from '@lib/code-editor-api';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';

// The node steps of the typed editor API (move, wrap, unwrap, delete, duplicate, snippet, re-indent) on the real CodeMirror editor
// the screen mounts, applied through `editor.apply` as a client of the API would. The exhaustive cases per operation and per language
// are the real-bundle contract (cm6Engine.contract.flow.test.tsx, C++ included, which the screen's picker cannot select); these are the
// calls that reach the same page through the screen, for Python and JavaScript. No operation has a button yet, so this is the whole proof.

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

beforeEach(() => useRunnerSettingStore.setState({ runner: 'backend' }));
afterEach(() => useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER }));

const eventually = (check: () => void | Promise<void>, timeout = 8_000) => waitFor(check, { timeout });

async function editorHolding(code: string, language: 'python' | 'javascript' = 'python'): Promise<FlowCodeEditor> {
  const ports: LeetCodePortOverrides = { gateway, voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort, audio: createFakeAudioRecorder() };
  const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
  const editor = await session.editor();
  if (language === 'javascript') {
    await userEvent.click(screen.getByTestId('ProblemEditorLanguagePicker-Trigger'));
    await waitFor(() => expect(screen.getByTestId('ProblemEditorLanguagePicker-JavaScript')).toBeTruthy());
    await userEvent.click(screen.getByTestId('ProblemEditorLanguagePicker-JavaScript'));
    await eventually(async () => expect((await editor.config()).language).toBe('javascript'));
  }
  await editor.setText(code);
  return editor;
}

// the whole-line node a statement is, as the editor's own tree names it
async function nodeAt(editor: FlowCodeEditor, code: string, needle: string): Promise<Range> {
  const node = await editor.statementAt(code.indexOf(needle) + 1);
  if (!node) throw new Error(`no statement at ${needle}`);
  return { from: node.from, to: node.to };
}

const PYTHON = 'def f(x):\n    if x:\n        a = 1\n        b = 2\n    return b\n';
const JAVASCRIPT = 'function f(x) {\n    if (x) {\n        a = 1;\n        b = 2;\n    }\n    return b;\n}\n';

describe('node steps through the problem editor', () => {
  it('given python code with an if holding two statements, when the second is moved after the if, then the editor text shows it at the if indent, the mirror agrees, and one undo restores the text', async () => {
    const editor = await editorHolding(PYTHON);
    const ifNode = await nodeAt(editor, PYTHON, 'if x:');
    const second = await nodeAt(editor, PYTHON, 'b = 2');

    const reply = await editor.apply([{ op: 'moveNode', node: second, to: { after: ifNode } }]);

    expect(reply.ok).toBe(true);
    expect(editor.text()).toBe('def f(x):\n    if x:\n        a = 1\n    b = 2\n    return b\n');
    expect(await editor.undo()).toBe(true);
    expect(editor.text()).toBe(PYTHON);
  });

  it.each([
    ['python', PYTHON, 'a = 1', 'b = 2', 'for i in range(2)', 'def f(x):\n    if x:\n        for i in range(2):\n            a = 1\n            b = 2\n    return b\n'],
    ['javascript', JAVASCRIPT, 'a = 1;', 'b = 2;', 'for (let i = 0; i < 2; i++)', 'function f(x) {\n    if (x) {\n        for (let i = 0; i < 2; i++) {\n            a = 1;\n            b = 2;\n        }\n    }\n    return b;\n}\n'],
  ] as const)('given two lines in %s, when they are wrapped in a for, then they sit one unit deeper under the header and one undo restores the text', async (language, code, first, last, header, wrapped) => {
    const editor = await editorHolding(code, language);
    const from = (await nodeAt(editor, code, first)).from;
    const to = (await nodeAt(editor, code, last)).to;

    const reply = await editor.apply([{ op: 'wrapNode', node: { from, to }, header }]);

    expect(reply.ok).toBe(true);
    expect(editor.text()).toBe(wrapped);
    expect(await editor.undo()).toBe(true);
    expect(editor.text()).toBe(code);
  });

  it('given a wrap header that does not parse, when it is applied, then the reply is would-not-parse and the editor text is unchanged', async () => {
    const editor = await editorHolding(PYTHON);
    const node = await nodeAt(editor, PYTHON, 'b = 2');

    const reply = await editor.apply([{ op: 'wrapNode', node, header: 'for i in' }]);

    expect(reply).toMatchObject({ ok: false, error: { code: 'would-not-parse', reason: 'syntax' } });
    expect(editor.text()).toBe(PYTHON);
  });

  it('given a deleteNode with preview, when it is applied, then the reply holds the would-be text and the revision and the editor text are unchanged', async () => {
    const editor = await editorHolding(PYTHON);
    const node = await nodeAt(editor, PYTHON, 'b = 2');

    const reply = await editor.apply([{ op: 'deleteNode', node }], { preview: true });

    expect(reply).toMatchObject({ ok: true, value: { changes: [], preview: { text: 'def f(x):\n    if x:\n        a = 1\n    return b\n' } } });
    expect(reply.revision).toBe(reply.ok ? reply.base : -1);
    expect(editor.text()).toBe(PYTHON);
  });

  it('given each other node step applied in turn, when it is undone, then the text before the call returns in one step', async () => {
    const code = 'def f(xs):\n    for x in xs:\n        print(x)\n        print(x)\n    return 1\n';
    const editor = await editorHolding(code);
    const loop = await nodeAt(editor, code, 'for x in xs:');
    const second = { from: code.lastIndexOf('print(x)'), to: code.lastIndexOf('print(x)') + 8 };
    const ret = await nodeAt(editor, code, 'return 1');
    const steps: readonly (readonly [string, Step])[] = [
      ['deleteNode', { op: 'deleteNode', node: second }],
      ['duplicateNode', { op: 'duplicateNode', node: ret }],
      ['unwrapNode', { op: 'unwrapNode', node: loop }],
      ['insertSnippet', { op: 'insertSnippet', at: { before: ret }, text: 'y = 2' }],
      ['reindentBlock', { op: 'reindentBlock', node: second, by: -1 }],
    ];

    for (const [name, step] of steps) {
      const reply = await editor.apply([step]);

      expect(reply.ok, name).toBe(true);
      expect(editor.text(), name).not.toBe(code);
      expect(await editor.undo(), name).toBe(true);
      expect(editor.text(), name).toBe(code);
    }
  });

  it('given a call at an older revision, when it is applied, then the reply is the stale failure and the helper returns instead of waiting for the mirror', async () => {
    const editor = await editorHolding(PYTHON);
    const node = await nodeAt(editor, PYTHON, 'b = 2');

    const reply = await editor.apply([{ op: 'deleteNode', node }], { at: 0 });

    expect(reply).toMatchObject({ ok: false, error: { code: 'stale' } });
    expect(editor.text()).toBe(PYTHON);
  });
});
