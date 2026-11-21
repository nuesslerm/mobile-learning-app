import { DependencyOverridesProvider } from '@lib/app-di';
import {
  act,
  createFakeAudioRecorder,
  createFakeVoiceEditTransport,
  FlowProviders,
  renderHook,
  waitFor,
} from '@lib/flow-test-harness';
import type { EditAlternative } from '@lib/voice-edit-dsl';
import type { ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';


import {
  usePlanRealize,
  type PlanRealizeOutcome,
} from '../usePlanRealize';
import { LeetCodeLiveContainer } from '../../../di/LeetCodeContainer';
import type { EditBasis, EditOutcome } from '../../../domain/editor/voiceEditPlan';
import { usePlanStore } from '../planStore';

const sendVoicePlan = vi.fn<(body: unknown) => Promise<unknown>>();
const sendEditStats = vi.fn(async (_row: unknown) => undefined);

// Mounted inside the leetcode composition root, stacked as the route layout stacks it, with the voice port spied.
function renderInLeetcode<T>(hook: () => T) {
  const voice = {
    ...createFakeVoiceEditTransport(),
    plan: (body: unknown) => sendVoicePlan(body),
    sendEditStats: (row: unknown) => sendEditStats(row),
  };
  const overrides = { leetcode: { audio: createFakeAudioRecorder(), voice } };
  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <FlowProviders>
        <DependencyOverridesProvider overrides={overrides}>
            <LeetCodeLiveContainer>{children}</LeetCodeLiveContainer>
        </DependencyOverridesProvider>
      </FlowProviders>
    );
  }
  return renderHook(hook, { wrapper: Wrapper });
}

const PROBLEM = 'valid-palindrome';
const STUB =
  'class Solution:\n    def isPalindrome(self, s: str) -> bool:\n        ';
const basisOf = (text: string, revision = 0): EditBasis => ({ text, revision, instance: 'page-1' });
const applied = (modifiedText: string): EditOutcome => ({ ok: true, modifiedText, landed: null, revision: 1 });

function plannerAnswer(commands: unknown) {
  return {
    transcript: 'plan',
    wallMs: 21,
    alternatives: [
      {
        id: 'alt-1',
        label: 'Answer',
        summary: 's',
        confidence: 0.9,
        commands,
      },
    ],
  };
}

describe('usePlanRealize', () => {
  beforeEach(() => {
    sendVoicePlan.mockReset();
    sendEditStats.mockClear();
    usePlanStore.setState({ plans: {} });
  });

  it('Given two pending steps on the palindrome stub and a planner replacing the stub line, When realize(step1) runs, Then the fenced answer is applied, the step is realized 3-3 and step 2 stays pending', async () => {
    // Given two pending steps on the stub
    usePlanStore.getState().addBlock(PROBLEM, 'Keep only alphanumeric characters');
    usePlanStore
      .getState()
      .addBlock(PROBLEM, 'Compare the cleaned string with its reverse');
    const [step1, step2] = usePlanStore.getState().stepsFor(PROBLEM);
    const applyEdit = vi.fn(async (_alt: EditAlternative, _basis: EditBasis) =>
      applied('class Solution:\n    def isPalindrome(self, s: str) -> bool:\n        return s == s[::-1]')
    );
    sendVoicePlan.mockResolvedValue(
      plannerAnswer([
        {
          type: 'replaceLines',
          startLine: 3,
          endLine: 3,
          newText: '        return s == s[::-1]',
        },
      ])
    );

    const { result } = renderInLeetcode(
      () =>
        usePlanRealize({
          problemId: PROBLEM,
          language: 'python3',
          basis: async () => basisOf(STUB),
          applyEdit,
        })
    );

    // When the first step is realised
    let outcome: PlanRealizeOutcome | undefined;
    await act(async () => {
      outcome = await result.current.realize(step1!.id);
    });

    // Then the planner got the fenced request about the basis text and the fenced answer reached applyEdit with that basis
    expect(outcome).toBe('applied');
    const body = sendVoicePlan.mock.calls[0]?.[0] as { transcript: string; currentCode: string };
    expect(body.currentCode).toBe(STUB);
    expect(applyEdit.mock.calls[0]?.[1]).toEqual(basisOf(STUB));
    expect(body.transcript).toContain('line 3');
    expect(body.transcript).toContain('Keep only alphanumeric characters');
    expect(applyEdit).toHaveBeenCalledTimes(1);
    expect(applyEdit.mock.calls[0]?.[0].commands).toEqual([
      {
        type: 'replaceLines',
        startLine: 3,
        endLine: 3,
        newText: '        return s == s[::-1]',
      },
    ]);

    // And the first step is realized on line 3 while the second is untouched
    await waitFor(() => {
      expect(usePlanStore.getState().stepsFor(PROBLEM)[0]).toMatchObject({
        status: 'realized',
        lines: { startLine: 3, endLine: 3 },
      });
    });
    const steps = usePlanStore.getState().stepsFor(PROBLEM);
    expect(steps[1]?.id).toBe(step2!.id);
    expect(steps[1]?.status).toBe('pending');
    expect(sendEditStats).toHaveBeenCalledWith(
      expect.objectContaining({
        source: 'plan-text',
        outcome: 'auto',
        planWallMs: 21,
        codeCharsBefore: STUB.length,
      })
    );
  });

  it('Given a planner answer that rewrites line 2, When realize(step1) runs, Then the result is invalid and nothing is applied', async () => {
    // Given two pending steps and an answer outside the step's fence
    usePlanStore.getState().addBlock(PROBLEM, 'First');
    usePlanStore.getState().addBlock(PROBLEM, 'Second');
    const [step1] = usePlanStore.getState().stepsFor(PROBLEM);
    const applyEdit = vi.fn(async (_alt: EditAlternative, _basis: EditBasis) => applied(STUB));
    sendVoicePlan.mockResolvedValue(
      plannerAnswer([
        {
          type: 'replaceLines',
          startLine: 2,
          endLine: 2,
          newText: '    def isPalindrome(self, s):',
        },
      ])
    );

    const { result } = renderInLeetcode(
      () =>
        usePlanRealize({
          problemId: PROBLEM,
          language: 'python3',
          basis: async () => basisOf(STUB),
          applyEdit,
        })
    );

    // When the first step is realised
    let outcome: PlanRealizeOutcome | undefined;
    await act(async () => {
      outcome = await result.current.realize(step1!.id);
    });

    // Then it is rejected and the buffer and plan are untouched
    expect(outcome).toBe('invalid');
    expect(applyEdit).not.toHaveBeenCalled();
    expect(usePlanStore.getState().stepsFor(PROBLEM)[0]?.status).toBe('pending');
  });

  it('Given an editor that refuses the edit as stale, When realize(step1) runs, Then the outcome is error, the step stays pending, the error shows and the stats row says error', async () => {
    // Given a valid answer that the editor refuses because the code moved on
    usePlanStore.getState().addBlock(PROBLEM, 'Keep only alphanumeric characters');
    const [step1] = usePlanStore.getState().stepsFor(PROBLEM);
    const applyEdit = vi.fn(async (_alt: EditAlternative, _basis: EditBasis): Promise<EditOutcome> => ({ ok: false, reason: 'stale', code: 'stale' }));
    sendVoicePlan.mockResolvedValue(plannerAnswer([{ type: 'replaceLines', startLine: 3, endLine: 3, newText: '        return True' }]));
    const { result } = renderInLeetcode(() => usePlanRealize({ problemId: PROBLEM, language: 'python3', basis: async () => basisOf(STUB), applyEdit }));

    // When the first step is realised
    let outcome: PlanRealizeOutcome | undefined;
    await act(async () => {
      outcome = await result.current.realize(step1!.id);
    });

    // Then nothing is realised, the person is told, and the turn is recorded as an error
    expect(outcome).toBe('error');
    expect(usePlanStore.getState().stepsFor(PROBLEM)[0]?.status).toBe('pending');
    expect(result.current.error).toBe('The code changed while the step was being planned. Realize it again.');
    expect(sendEditStats).toHaveBeenCalledWith(
      expect.objectContaining({ outcome: 'error', error: 'The code changed while the step was being planned. Realize it again.' })
    );
  });

  it('Given two steps realised back to back, When the first lands, Then the second plans against the text that includes the first', async () => {
    // Given an editor whose text moves when the first edit lands
    usePlanStore.getState().addBlock(PROBLEM, 'Keep only alphanumeric characters');
    usePlanStore.getState().addBlock(PROBLEM, 'Compare the cleaned string with its reverse');
    const [step1, step2] = usePlanStore.getState().stepsFor(PROBLEM);
    const FIRST = `${STUB}cleaned = s`;
    let text = STUB;
    sendVoicePlan.mockResolvedValue(plannerAnswer([{ type: 'replaceLines', startLine: 3, endLine: 3, newText: '        cleaned = s' }]));
    const applyEdit = vi.fn(async (_alt: EditAlternative, _basis: EditBasis) => {
      text = FIRST;
      return applied(FIRST);
    });
    const { result } = renderInLeetcode(() => usePlanRealize({ problemId: PROBLEM, language: 'python3', basis: async () => basisOf(text), applyEdit }));

    // When the two steps are realised one after the other
    await act(async () => {
      await result.current.realize(step1!.id);
    });
    sendVoicePlan.mockResolvedValue(plannerAnswer([{ type: 'insertAfterLine', line: 3, text: '        return cleaned == cleaned[::-1]' }]));
    await act(async () => {
      await result.current.realize(step2!.id);
    });

    // Then the second plan was asked with the text the first edit made
    expect(sendVoicePlan.mock.calls.map(([params]) => (params as { currentCode: string }).currentCode)).toEqual([STUB, FIRST]);
    expect(applyEdit.mock.calls.map(([, basis]) => basis.text)).toEqual([STUB, FIRST]);
  });

  it('Given an editor that cannot be read, When realize(step1) runs, Then the planner is never asked and the error says the editor is not ready', async () => {
    // Given a basis reader with nothing behind it
    usePlanStore.getState().addBlock(PROBLEM, 'Keep only alphanumeric characters');
    const [step1] = usePlanStore.getState().stepsFor(PROBLEM);
    const applyEdit = vi.fn(async (_alt: EditAlternative, _basis: EditBasis) => applied(STUB));
    const { result } = renderInLeetcode(() => usePlanRealize({ problemId: PROBLEM, language: 'python3', basis: async () => null, applyEdit }));

    // When the first step is realised
    let outcome: PlanRealizeOutcome | undefined;
    await act(async () => {
      outcome = await result.current.realize(step1!.id);
    });

    // Then nothing was asked or applied
    expect(outcome).toBe('error');
    expect(sendVoicePlan).not.toHaveBeenCalled();
    expect(applyEdit).not.toHaveBeenCalled();
    expect(result.current.error).toBe('The editor is not ready. Try again.');
  });
});
