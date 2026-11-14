import {
  act,
  createFakeAudioRecorder,
  FlowProviders,
  renderHook,
  waitFor,
} from '@lib/flow-test-harness';
import { DependencyOverridesProvider } from '@lib/app-di';
import type { ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { VoiceEditTransportPort } from '../../domain/ports';
import { LeetCodeLiveContainer } from '../../di/LeetCodeContainer';
import { useVoiceEdit } from '../useVoiceEdit';

const STUB = "class Solution:\n    def isPalindrome(self, s: str) -> bool:\n        ";

const plan = vi.fn<VoiceEditTransportPort['plan']>();
const transcribe = vi.fn<VoiceEditTransportPort['transcribe']>();
const sendEditStats = vi.fn<VoiceEditTransportPort['sendEditStats']>();

const voice = { transcribe, plan, sendEditStats };
const audio = createFakeAudioRecorder();
const gateway = {
  userStatus: vi.fn(),
  test: vi.fn(),
  submit: vi.fn(),
  poll: vi.fn(),
  runLocal: vi.fn(),
  format: vi.fn(),
  loginCaptureStatus: vi.fn(),
};

// app.renderHook's fixed wrapper cannot mount the leetcode composition root,
// which useVoiceRecording needs via useLeetCodePorts; boot it here, stacked
// the same way the leetcode route layout stacks it. expo-audio is covered by
// the harness stub, so no vi.mock stays for it.
const overrides = { leetcode: { gateway, voice, audio } };

function Wrapper({ children }: { children: ReactNode }) {
  return (
    <FlowProviders>
      <DependencyOverridesProvider overrides={overrides}>
          <LeetCodeLiveContainer>{children}</LeetCodeLiveContainer>
      </DependencyOverridesProvider>
    </FlowProviders>
  );
}

function typedTurn(result: { current: ReturnType<typeof useVoiceEdit> }, text: string) {
  act(() => result.current.startTextEdit(text));
  return act(async () => {
    await result.current.confirmSend();
  });
}

describe('useVoiceEdit text edits', () => {
  beforeEach(() => {
    plan.mockReset();
    transcribe.mockClear();
    sendEditStats.mockClear();
  });

  it('given a typed instruction, when sent, then it goes to the planner', async () => {
    // Given a planner that answers with one viable alternative
    plan.mockResolvedValue({
      transcript: 'echo',
      wallMs: 12,
      alternatives: [
        {
          id: 'alt-1', label: 'Add loop', summary: 's', confidence: 0.95,
          commands: [
            { type: 'replaceLines', startLine: 3, endLine: 3, newText: '        while left < right:\n            pass' },
          ],
        },
      ],
    });
    const { result } = renderHook(() => useVoiceEdit(STUB, 'python3', 'p.m4a'), {
      wrapper: Wrapper,
    });

    // When the instruction is typed and sent
    await typedTurn(result, 'add a while loop that runs while left is less than right');

    // Then the planner got the typed words, marked as a text edit
    await waitFor(() => expect(plan).toHaveBeenCalledTimes(1));
    const body = plan.mock.calls[0]?.[0] as { transcript: string; source?: string };
    expect(body.transcript).toContain('left is less than right');
    expect(body.source).toBe('text');

    // And the alternative lands and is applied
    await waitFor(() => expect(result.current.state.phase).not.toBe('planning'));
    expect(result.current.state.alternatives[0]).toMatchObject({ id: 'alt-1' });
  });

  it('given a typed rename, when sent, then it takes the local path without a planner call', async () => {
    // Given the starter code containing the symbol s
    const { result } = renderHook(() => useVoiceEdit(STUB, 'python3', 'p.m4a'), {
      wrapper: Wrapper,
    });

    // When the rename is typed and sent
    await typedTurn(result, 'rename the variable s to ch everywhere');

    // Then the auto-applied alternative is the local rename, without a planner call
    await waitFor(() => expect(result.current.state.phase).toBe('applied'));
    const applied = result.current.consumeAutoApply();
    expect(applied?.id).toBe('local-rename');
    expect(applied?.commands).toEqual([
      { type: 'renameSymbol', position: expect.any(Number), newName: 'ch' },
    ]);
    expect(plan).not.toHaveBeenCalled();
  });
});