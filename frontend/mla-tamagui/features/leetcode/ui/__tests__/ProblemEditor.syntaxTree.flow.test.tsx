import { createFakeAudioRecorder, createFakeVoiceEditTransport, type FlowCodeEditor } from '@lib/flow-test-harness';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { app } from '../../../../tests/flow/app';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../state/runnerSettingStore';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';

// The syntax tree of the real CodeMirror editor, read through the flow harness: the answers the editor's next features (indent after a
// block, completion in scope) will rest on, asserted on known Python snippets. Python only: the screen inits Python until a switch.

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

async function editorHolding(code: string): Promise<FlowCodeEditor> {
  const ports: LeetCodePortOverrides = { gateway, voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort, audio: createFakeAudioRecorder() };
  const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
  const editor = await session.editor();
  await editor.setText(code);
  return editor;
}

describe('the syntax tree of the problem editor', () => {
  it('given a def with a for inside, when the caret is in the for body, then the enclosing block is the for body and its owner is the for', async () => {
    const code = 'def f(xs):\n    for x in xs:\n        print(x)\n    return 1\n';
    const editor = await editorHolding(code);

    const inside = await editor.enclosingBlock(code.indexOf('print') + 2);
    const onTheLine = await editor.enclosingBlock(code.indexOf('for x') + 3);

    expect(inside?.owner?.type).toBe('ForStatement');
    expect(inside?.block.type).toBe('Body');
    expect(onTheLine?.owner?.type).toBe('FunctionDefinition');
    expect((await editor.tree()).complete).toBe(true);
  });

  it('given a half-typed for line, when the tree is asked, then the line is an error region and the def around it is still found', async () => {
    const code = 'def f(xs):\n    for x in\n    return 1\n';
    const editor = await editorHolding(code);

    const tree = await editor.tree();
    const block = await editor.enclosingBlock(code.indexOf('return') + 3);

    expect(JSON.stringify(tree.root)).toContain('"error":true');
    expect(block?.owner?.type).toBe('FunctionDefinition');
  });

  it('given a decorated async def, when the caret is on the decorator and in the body, then the statement is the decorated one and the owner of the body is the function', async () => {
    const code = '@cache\nasync def f():\n    return 1\n';
    const editor = await editorHolding(code);

    const decorator = await editor.statementAt(2);
    const block = await editor.enclosingBlock(code.indexOf('return') + 2);

    expect(decorator?.type).toBe('DecoratedStatement');
    expect(block?.owner?.type).toBe('FunctionDefinition');
  });
});
