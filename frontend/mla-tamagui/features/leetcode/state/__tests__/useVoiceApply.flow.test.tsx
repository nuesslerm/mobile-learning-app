import { createEditorClient } from '@lib/code-editor-api';
import { createFakeEditor } from '@lib/code-editor-api/testing';
import { DependencyOverridesProvider } from '@lib/app-di';
import { act, createFakeAudioRecorder, FlowProviders, renderHook, waitFor } from '@lib/flow-test-harness';
import type { EditAlternative } from '@lib/voice-edit-dsl';
import type { ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { LeetCodeLiveContainer } from '../../di/LeetCodeContainer';
import type { VoiceEditTransportPort } from '../../domain/ports';
import { applyPlannedEdit, readBasis, type EditTarget } from '../../domain/editor/voiceEditPlan';
import { STALE_EDIT, useVoiceApply } from '../useVoiceApply';
import { useVoiceEdit } from '../useVoiceEdit';

const STARTER = 'class Solution:\n    def isPalindrome(self, s: str) -> bool:\n        ';

const plan = vi.fn<VoiceEditTransportPort['plan']>();
const sendEditStats = vi.fn<VoiceEditTransportPort['sendEditStats']>();
const voice = { transcribe: vi.fn(), plan, sendEditStats };
const gateway = {
  userStatus: vi.fn(),
  test: vi.fn(),
  submit: vi.fn(),
  poll: vi.fn(),
  runLocal: vi.fn(),
  format: vi.fn(),
  loginCaptureStatus: vi.fn(),
};
const overrides = { leetcode: { gateway, voice, audio: createFakeAudioRecorder() } };

function Wrapper({ children }: { children: ReactNode }) {
  return (
    <FlowProviders>
      <DependencyOverridesProvider overrides={overrides}>
        <LeetCodeLiveContainer>{children}</LeetCodeLiveContainer>
      </DependencyOverridesProvider>
    </FlowProviders>
  );
}

const alternative = (id: string, confidence: number, text: string) => ({
  id,
  label: id,
  summary: id,
  confidence,
  commands: [{ type: 'insertAfterLine' as const, line: 3, text }],
});
const answer = (...alternatives: ReturnType<typeof alternative>[]) => ({ transcript: 'echo', wallMs: 5, alternatives });

// A turn against a real editor engine: the hook plans and applies through the same target a screen gives it.
function mount(overrides: Partial<EditTarget> = {}) {
  const fake = createFakeEditor({ text: STARTER, instance: 'page-1' });
  const client = createEditorClient(fake.engine);
  const target: EditTarget = { basis: () => readBasis(client), applyEdit: (edit) => applyPlannedEdit(client, edit), ...overrides };
  const snapshot = vi.fn<(reason: 'voice-edit', code: string) => void>();
  const rendered = renderHook(
    () => {
      const voiceEdit = useVoiceEdit(STARTER, 'python3', 'p.m4a', { readBasis: target.basis });
      return { voiceEdit, ...useVoiceApply({ target, voiceEdit, snapshot, problemId: 'valid-palindrome' }) };
    },
    { wrapper: Wrapper }
  );
  return { fake, snapshot, ...rendered };
}
type Mounted = ReturnType<typeof mount>;

async function sendTypedInstruction({ result }: Mounted) {
  act(() => result.current.voiceEdit.startTextEdit('declare the result'));
  await act(async () => {
    await result.current.voiceEdit.confirmSend();
  });
  await waitFor(() => expect(['applied', 'choosing']).toContain(result.current.voiceEdit.state.phase));
}

const applyFirst = ({ result }: Mounted) =>
  act(async () => {
    const alt = result.current.voiceEdit.consumeAutoApply() as EditAlternative;
    await result.current.apply(alt);
  });

describe('useVoiceApply', () => {
  beforeEach(() => {
    plan.mockReset();
    sendEditStats.mockClear();
  });

  it('given a plan that auto-applies and an editor that has not moved, when it is applied, then the edit landed, history holds the replaced text once and the stats row says auto', async () => {
    // Given
    plan.mockResolvedValue(answer(alternative('alt-1', 0.95, '        return True')));
    const turn = mount();
    await sendTypedInstruction(turn);

    // When
    await applyFirst(turn);

    // Then
    expect(turn.fake.text()).toBe(`${STARTER}return True`);
    expect(turn.snapshot).toHaveBeenCalledTimes(1);
    expect(turn.snapshot).toHaveBeenCalledWith('voice-edit', STARTER);
    expect(turn.result.current.voiceEdit.state.phase).toBe('idle');
    expect(sendEditStats).toHaveBeenCalledTimes(1);
    expect(sendEditStats.mock.calls[0]?.[0]).toMatchObject({ outcome: 'auto', pickedIndex: null, codeCharsAfter: `${STARTER}return True`.length });
  });

  it('given two alternatives to choose from, when the second is picked, then the stats row says picked at index 1', async () => {
    // Given a plan with no clear winner
    plan.mockResolvedValue(answer(alternative('alt-1', 0.6, '        return 1'), alternative('alt-2', 0.5, '        return 2')));
    const turn = mount();
    await sendTypedInstruction(turn);
    expect(turn.result.current.voiceEdit.state.phase).toBe('choosing');

    // When the second row is picked
    await act(async () => {
      await turn.result.current.apply(turn.result.current.voiceEdit.state.alternatives[1] as EditAlternative);
    });

    // Then
    expect(turn.fake.text()).toBe(`${STARTER}return 2`);
    expect(sendEditStats.mock.calls[0]?.[0]).toMatchObject({ outcome: 'picked', pickedIndex: 1 });
  });

  it('given a keystroke typed after the plan was made, when it is applied, then it is refused once with the banner text, the typed text stays and no history is taken', async () => {
    // Given a planned turn and a keystroke that reached the editor afterwards
    plan.mockResolvedValue(answer(alternative('alt-1', 0.95, '        return True')));
    const turn = mount();
    await sendTypedInstruction(turn);
    turn.fake.userTypes('x');

    // When the plan is applied
    await applyFirst(turn);

    // Then the turn ended in the error phase with the stale copy, once, and nothing was applied or recorded
    await waitFor(() => expect(turn.result.current.voiceEdit.state.phase).toBe('error'));
    expect(turn.result.current.voiceEdit.state.error).toBe(STALE_EDIT);
    expect(turn.fake.text()).toBe(`x${STARTER}`);
    expect(turn.snapshot).not.toHaveBeenCalled();
    expect(sendEditStats).toHaveBeenCalledTimes(1);
    expect(sendEditStats.mock.calls[0]?.[0]).toMatchObject({ outcome: 'error', error: STALE_EDIT });
  });

  it('given an editor that is not ready, when an alternative is applied, then the editor-not-ready copy is shown and no history is taken', async () => {
    // Given a planned turn whose editor answers not-ready to the apply
    plan.mockResolvedValue(answer(alternative('alt-1', 0.95, '        return True')));
    const turn = mount({ applyEdit: async () => ({ ok: false, reason: 'not-ready', code: 'not-ready' }) });
    await sendTypedInstruction(turn);

    // When it is applied
    await applyFirst(turn);

    // Then
    await waitFor(() => expect(turn.result.current.voiceEdit.state.phase).toBe('error'));
    expect(turn.result.current.voiceEdit.state.error).toBe('The editor is not ready. Try again.');
    expect(turn.snapshot).not.toHaveBeenCalled();
  });
});
