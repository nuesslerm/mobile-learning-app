import { createFakeAudioRecorder, createFakeVoiceEditTransport, screen, userEvent, waitFor } from '@lib/flow-test-harness';
import { beforeEach, describe, expect, it } from 'vitest';

import { app } from '../../../../../tests/flow/app';
import type { LeetCodePortOverrides, RunResponse, VoiceEditTransportPort } from '../../../index';
import type { TestResult } from '../../../domain/run/execution';
import { createFakeCodeRunner } from '../../../testing/index';
import { exists, fakeGateway, resetConsoleTwin, tap, text, verdict } from './consoleTwinSupport';

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
const responseOf = (results: TestResult[]): RunResponse => {
  const correct = results.filter((entry) => entry.pass).length;
  return { source: 'offline', kind: 'test', state: 'FAILURE', requestId: 'cases', run_success: true, status_msg: 'Wrong Answer', total_correct: correct, total_testcases: results.length, results };
};

async function submitWith(results: TestResult[]) {
  const ports: LeetCodePortOverrides = { gateway: fakeGateway(), voice: createFakeVoiceEditTransport() as unknown as VoiceEditTransportPort, audio: createFakeAudioRecorder(), runner: createFakeCodeRunner(() => responseOf(results)) };
  const session = await app.render('/leetcode/two-sum', { overrides: { leetcode: ports } });
  await session.editor();
  await tap('ConsoleBarSubmit');
  await waitFor(() => expect(exists('ConsoleResultVerdict')).toBe(true));
}

beforeEach(() => resetConsoleTwin('two-sum'));

describe('the console result shows the first failing case in full', () => {
  it('Given a run with passing and failing cases, When the result shows, Then the tally and the first failing case read with its input, output and expected', async () => {
    await submitWith([result(true), result(false, { actual: [1, 2], elapsed_ms: 7 }), result(false, { actual: null, error: 'IndexError: list index out of range' })]);

    expect(verdict()).toBe('Wrong Answer');
    expect(text('ConsoleResultCounts')).toBe('1 / 3 testcases passed');
    expect(text('ConsoleResultInput-nums')).toBe('[2,7,11,15]');
    expect(text('ConsoleResultInput-target')).toBe('9');
    expect(text('ConsoleResultOutput')).toBe('[1,2]');
    expect(text('ConsoleResultExpected')).toBe('[0,1]');
  });

  it('Given a case that raised and printed, When the result shows, Then the error, the input that ran and what it printed are on it and there is no output card', async () => {
    await submitWith([result(true), result(false, { actual: null, error: 'IndexError: list index out of range', stdout: 'debug line' })]);

    expect(verdict()).toBe('Runtime Error');
    expect(text('ConsoleResultErrorText')).toBe('IndexError: list index out of range');
    expect(text('ConsoleResultStdout')).toBe('debug line');
    expect(text('ConsoleResultLast-nums')).toBe('[2,7,11,15]');
    expect(exists('ConsoleResultOutput')).toBe(false);
  });

  it('Given a case with a long argument, When the result shows and the value is tapped, Then it is clipped to a line and opens in full', async () => {
    await submitWith([result(false, { input: { nums: LONG, target: 9 }, actual: [1, 2] })]);
    const field = () => text('ConsoleResultInput-nums') ?? '';
    expect(field().endsWith('…')).toBe(true);
    expect(field().length).toBeLessThan(100);

    await userEvent.click(screen.getByTestId('ConsoleResultInput-nums-Toggle'));
    expect(field()).toContain('39000');

    await userEvent.click(screen.getByTestId('ConsoleResultInput-nums-Toggle'));
    expect(field().endsWith('…')).toBe(true);
  });
});
