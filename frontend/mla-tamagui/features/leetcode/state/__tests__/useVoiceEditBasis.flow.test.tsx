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
import type { EditBasis } from '../../domain/editor/voiceEditPlan';
import { LeetCodeLiveContainer } from '../../di/LeetCodeContainer';
import { useVoiceEdit } from '../useVoiceEdit';

// What the screen's React state still holds: one keystroke behind the editor.
const REACT_COPY = 'class Solution:\n    def isPalindrome(self, s: str) -> bool:\n        ';
const EDITOR_TEXT = `${REACT_COPY}left = 0`;
const NOT_READY = 'The editor is not ready. Try again.';

const plan = vi.fn<VoiceEditTransportPort['plan']>();
const transcribe = vi.fn<VoiceEditTransportPort['transcribe']>();
const sendEditStats = vi.fn<VoiceEditTransportPort['sendEditStats']>();

const voice = { transcribe, plan, sendEditStats };
const audio = createFakeAudioRecorder({ durationMs: 1200 });
const gateway = {
  userStatus: vi.fn(),
  test: vi.fn(),
  submit: vi.fn(),
  poll: vi.fn(),
  runLocal: vi.fn(),
  format: vi.fn(),
  loginCaptureStatus: vi.fn(),
};
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

const basisAt = (revision: number, text = EDITOR_TEXT): EditBasis => ({ text, revision, instance: 'page-1' });

const answer = (id: string, transcript = 'echo') => ({
  transcript,
  wallMs: 12,
  alternatives: [
    {
      id,
      label: 'Declare left',
      summary: 's',
      confidence: 0.95,
      commands: [{ type: 'insertAfterLine' as const, line: 3, text: '        right = 1' }],
    },
  ],
});

function render(readBasis?: () => Promise<EditBasis | null>) {
  return renderHook(
    () => useVoiceEdit(REACT_COPY, 'python3', 'p.m4a', readBasis ? { readBasis } : {}),
    { wrapper: Wrapper }
  );
}
type Rendered = ReturnType<typeof render>['result'];

function typedTurn(result: Rendered, text: string) {
  act(() => result.current.startTextEdit(text));
  return act(async () => {
    await result.current.confirmSend();
  });
}

async function spokenTurnToReview(result: Rendered, transcript: string) {
  transcribe.mockResolvedValue({ transcript, wallMs: 5 });
  await act(async () => {
    await result.current.start();
  });
  await act(async () => {
    await result.current.stop();
  });
  await waitFor(() => expect(result.current.state.phase).toBe('reviewing'));
}

describe('useVoiceEdit planning against the editor basis', () => {
  beforeEach(() => {
    plan.mockReset();
    transcribe.mockReset();
    sendEditStats.mockClear();
  });

  it("given a basis reader reporting the editor's text at revision 7, when a typed instruction is sent, then the planner is asked with that text and not the React copy", async () => {
    // Given a reader that answers with text a keystroke ahead of the React copy
    plan.mockResolvedValue(answer('alt-1'));
    const { result } = render(vi.fn().mockResolvedValue(basisAt(7)));

    // When the instruction is typed and sent
    await typedTurn(result, 'declare right as one');

    // Then the planner saw the editor's text
    await waitFor(() => expect(plan).toHaveBeenCalledTimes(1));
    expect(plan.mock.calls[0]?.[0]).toMatchObject({ currentCode: EDITOR_TEXT, transcript: 'declare right as one' });
  });

  it('given a basis reader and a typed rename, when sent, then the local alternative is parsed against the basis text and the state carries that basis', async () => {
    // Given a name that only the editor's text holds (the React copy has no `left`)
    const basis = basisAt(7);
    const { result } = render(vi.fn().mockResolvedValue(basis));

    // When the rename is typed and sent
    await typedTurn(result, 'rename the variable left to lo');

    // Then the rename was found in the basis text, without a planner call, and the basis rides in the state
    await waitFor(() => expect(result.current.state.phase).toBe('applied'));
    expect(result.current.state.basis).toEqual(basis);
    expect(result.current.consumeAutoApply()?.commands).toEqual([
      { type: 'renameSymbol', position: EDITOR_TEXT.indexOf('left'), newName: 'lo' },
    ]);
    expect(plan).not.toHaveBeenCalled();
  });

  it('given a basis reader, when the plan comes back, then the state carries the basis the plan was made for beside the alternatives', async () => {
    // Given a planner that answers with one viable alternative
    plan.mockResolvedValue(answer('alt-1'));
    const basis = basisAt(7);
    const { result } = render(vi.fn().mockResolvedValue(basis));

    // When a typed instruction is sent
    await typedTurn(result, 'declare right as one');

    // Then the applied state names the alternative and its basis
    await waitFor(() => expect(result.current.state.phase).toBe('applied'));
    expect(result.current.state.basis).toEqual(basis);
    expect(result.current.state.autoApply).toMatchObject({ id: 'alt-1' });
  });

  it('given a speculative plan made at revision 7, when the editor reports revision 8 at Send, then a new plan is asked and the old answer is dropped', async () => {
    // Given a spoken turn whose speculative plan was made at revision 7
    plan.mockResolvedValueOnce(answer('old')).mockResolvedValueOnce(answer('fresh'));
    const readBasis = vi.fn<() => Promise<EditBasis | null>>().mockResolvedValueOnce(basisAt(7)).mockResolvedValue(basisAt(8));
    const { result } = render(readBasis);
    await spokenTurnToReview(result, 'declare right as one');
    await waitFor(() => expect(plan).toHaveBeenCalledTimes(1));

    // When the person sends after the editor moved on
    await act(async () => {
      await result.current.confirmSend();
    });

    // Then the plan was asked again, and the state holds the new answer at the new revision
    await waitFor(() => expect(result.current.state.phase).toBe('applied'));
    expect(plan).toHaveBeenCalledTimes(2);
    expect(result.current.state.autoApply).toMatchObject({ id: 'fresh' });
    expect(result.current.state.basis?.revision).toBe(8);
  });

  it('given the same revision and transcript, when sent, then the speculative plan is reused', async () => {
    // Given a spoken turn whose speculative plan was made at revision 7 and an editor that has not moved
    plan.mockResolvedValue(answer('speculative'));
    const { result } = render(vi.fn().mockResolvedValue(basisAt(7)));
    await spokenTurnToReview(result, 'declare right as one');
    await waitFor(() => expect(plan).toHaveBeenCalledTimes(1));

    // When the person sends
    await act(async () => {
      await result.current.confirmSend();
    });

    // Then no second plan was asked and the speculative answer is the one applied
    await waitFor(() => expect(result.current.state.phase).toBe('applied'));
    expect(plan).toHaveBeenCalledTimes(1);
    expect(result.current.state.autoApply).toMatchObject({ id: 'speculative' });
  });

  it('given a basis reader that returns null, when sent, then the phase is error with the not-ready message and the planner was never asked', async () => {
    // Given an editor that cannot be read
    const { result } = render(vi.fn().mockResolvedValue(null));

    // When a typed instruction is sent
    await typedTurn(result, 'declare right as one');

    // Then the turn ends in the error phase before any plan call, and its stats row says error
    await waitFor(() => expect(result.current.state.phase).toBe('error'));
    expect(result.current.state.error).toBe(NOT_READY);
    expect(plan).not.toHaveBeenCalled();
    expect(sendEditStats).toHaveBeenCalledTimes(1);
    expect(sendEditStats.mock.calls[0]?.[0]).toMatchObject({ outcome: 'error', error: NOT_READY });
  });

  it("given a turn in the applied phase, when the screen reports a refused apply, then the phase is error and the stats row has outcome 'error' with that text", async () => {
    // Given a typed turn that reached the applied phase
    plan.mockResolvedValue(answer('alt-1'));
    const { result } = render(vi.fn().mockResolvedValue(basisAt(7)));
    await typedTurn(result, 'declare right as one');
    await waitFor(() => expect(result.current.state.phase).toBe('applied'));

    // When the screen says the apply was refused
    act(() => result.current.fail('The code changed while the edit was being planned. Say it again.'));

    // Then the banner phase shows the text, nothing is left to apply, and the stats row has the reason
    expect(result.current.state.phase).toBe('error');
    expect(result.current.state.error).toBe('The code changed while the edit was being planned. Say it again.');
    expect(result.current.consumeAutoApply()).toBeNull();
    expect(sendEditStats).toHaveBeenCalledTimes(1);
    expect(sendEditStats.mock.calls[0]?.[0]).toMatchObject({
      outcome: 'error',
      error: 'The code changed while the edit was being planned. Say it again.',
    });
  });

  it('given no basis reader, when a typed instruction is sent, then the planner is asked with the React copy and the state carries no basis', async () => {
    // Given an old caller that passes no reader
    plan.mockResolvedValue(answer('alt-1'));
    const { result } = render();

    // When a typed instruction is sent
    await typedTurn(result, 'declare right as one');

    // Then it behaves as before: the React text goes to the planner and there is no basis
    await waitFor(() => expect(result.current.state.phase).toBe('applied'));
    expect(plan.mock.calls[0]?.[0]).toMatchObject({ currentCode: REACT_COPY });
    expect(result.current.state.basis).toBeNull();
  });
});
