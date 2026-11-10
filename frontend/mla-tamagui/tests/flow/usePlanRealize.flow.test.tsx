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
} from '../../features/leetcode/plan/hooks/usePlanRealize';
import { ExpoAudioRecorderProvider } from '../../features/leetcode/data/ExpoAudioRecorderAdapter';
import { LeetCodeLiveContainer } from '../../features/leetcode/di/LeetCodeContainer';
import { usePlanStore } from '../../features/leetcode/plan/state/planStore';

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
          <ExpoAudioRecorderProvider>
            <LeetCodeLiveContainer>{children}</LeetCodeLiveContainer>
          </ExpoAudioRecorderProvider>
        </DependencyOverridesProvider>
      </FlowProviders>
    );
  }
  return renderHook(hook, { wrapper: Wrapper });
}

const PROBLEM = 'valid-palindrome';
const STUB =
  'class Solution:\n    def isPalindrome(self, s: str) -> bool:\n        ';

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
    const applyEdit = vi.fn((_alt: EditAlternative) => ({
      modifiedText:
        'class Solution:\n    def isPalindrome(self, s: str) -> bool:\n        return s == s[::-1]',
    }));
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
          getCode: () => STUB,
          applyEdit,
        })
    );

    // When the first step is realised
    let outcome: PlanRealizeOutcome | undefined;
    await act(async () => {
      outcome = await result.current.realize(step1!.id);
    });

    // Then the planner got the fenced request and the fenced answer reached applyEdit
    expect(outcome).toBe('applied');
    const body = sendVoicePlan.mock.calls[0]?.[0] as { transcript: string };
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
    const applyEdit = vi.fn((_alt: EditAlternative) => ({
      modifiedText: STUB,
    }));
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
          getCode: () => STUB,
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
});
