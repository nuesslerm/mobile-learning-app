import { createFakeAudioRecorder, createFakeVoiceEditTransport, screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { describe, expect, it, vi } from 'vitest';

import { app } from '../../../../tests/flow/app';
import type { LeetCodeGatewayPort, LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../..';
import type { TestResult } from '../../domain/execution';
import { createFakeCodeRunner } from '../../testing';

const LONG = Array.from({ length: 40 }, (_, i) => i * 1000);
const result = (pass: boolean, over: Partial<TestResult> = {}): TestResult => ({
  pass,
  input: { nums: [2, 7, 11, 15], target: 9 },
  actual: [0, 1],
  expected: [0, 1],
  stdout: '',
  stderr: '',
  error: null,
  elapsed_ms: 3,
  ...over,
});
const RESULTS = [
  result(true),
  result(false, { actual: [1, 2], elapsed_ms: 7 }),
  result(true, { input: { nums: LONG, target: 9 } }),
  result(false, { actual: null, error: 'IndexError: list index out of range', stdout: 'debug line' }),
];
const MIXED: RunResponse = { source: 'offline', kind: 'test', state: 'FAILURE', requestId: 'cases', run_success: true, status_msg: 'Wrong Answer', total_correct: 2, total_testcases: 4, results: RESULTS };

async function renderWithRun() {
  const gateway = { userStatus: vi.fn(async () => ({ isSignedIn: true, username: 'u' })), runLocal: vi.fn(), test: vi.fn(), submit: vi.fn(), poll: vi.fn(), format: vi.fn(async (p: { code: string }) => p.code), loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })) } as unknown as LeetCodeGatewayPort;
  const ports: LeetCodePortOverrides = { gateway, voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort, audio: createFakeAudioRecorder(), runner: createFakeCodeRunner(() => MIXED) };
  const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
  await session.editor();
  await userEvent.click(screen.getByTestId('ProblemEditorRunLocalButton'));
  await waitFor(() => expect(screen.getByTestId('ProblemEditorRunCases')).toBeTruthy());
}

describe('the run card shows every case', () => {
  it('Given a run with passing and failing cases, When the card shows, Then the failing cases come first with their input, expected, actual, error, output and time', async () => {
    await renderWithRun();

    // Then the failures are listed before the passes
    const order = screen.getAllByTestId(/^ProblemEditorRunCase-\d+$/).map((el) => el.getAttribute('data-testid'));
    expect(order).toEqual(['ProblemEditorRunCase-1', 'ProblemEditorRunCase-3', 'ProblemEditorRunCase-0', 'ProblemEditorRunCase-2']);
    // And a failed case reads in full
    expect(screen.getByTestId('ProblemEditorRunCase-1-Verdict').textContent).toBe('Case 2 · failed');
    expect(screen.getByTestId('ProblemEditorRunCase-1-Input').textContent).toBe('nums = [2,7,11,15], target = 9');
    expect(screen.getByTestId('ProblemEditorRunCase-1-Expected').textContent).toBe('[0,1]');
    expect(screen.getByTestId('ProblemEditorRunCase-1-Actual').textContent).toBe('[1,2]');
    expect(screen.getByTestId('ProblemEditorRunCase-1-Time').textContent).toBe('7 ms');
    // And a case that raised shows the error and what it printed, with no value for what it returned
    expect(screen.getByTestId('ProblemEditorRunCase-3-Error').textContent).toBe('IndexError: list index out of range');
    expect(screen.getByTestId('ProblemEditorRunCase-3-Stdout').textContent).toBe('debug line');
    expect(screen.getByTestId('ProblemEditorRunCase-3-Actual').textContent).toBe('—');
  });

  it('Given a case with a long argument, When the card shows and the value is tapped, Then it is clipped to a line and opens in full', async () => {
    await renderWithRun();
    const field = () => screen.getByTestId('ProblemEditorRunCase-2-Input').textContent ?? '';
    expect(field().endsWith('…')).toBe(true);
    expect(field().length).toBeLessThan(100);

    await userEvent.click(screen.getByTestId('ProblemEditorRunCase-2-Input-Toggle'));
    expect(field()).toContain('39000');

    await userEvent.click(screen.getByTestId('ProblemEditorRunCase-2-Input-Toggle'));
    expect(field().endsWith('…')).toBe(true);
  });
});
