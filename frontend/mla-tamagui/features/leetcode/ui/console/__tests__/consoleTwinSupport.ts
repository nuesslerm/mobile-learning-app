import { screen, userEvent } from '@lib/flow-test-harness';
import { vi } from 'vitest';

import type { LeetCodeGatewayPort, RunResponse } from '../../..';
import { ProblemCatalog } from '../../../domain/problemCatalog';
import { useConsoleCasesStore } from '../../../state/console/consoleCasesStore';
import { DEFAULT_RUNNER, useRunnerSettingStore } from '../../../state/runnerSettingStore';

export const BACKEND_RUN: RunResponse = { source: 'backend-run', kind: 'test', state: 'SUCCESS', requestId: 'backend', run_success: true, status_msg: 'Accepted', total_correct: 5, total_testcases: 5 };

export function fakeGateway(over: Partial<LeetCodeGatewayPort> = {}): LeetCodeGatewayPort {
  return {
    userStatus: vi.fn(async () => ({ isSignedIn: true, username: 'flow-user' })),
    test: vi.fn(async () => BACKEND_RUN),
    submit: vi.fn(async () => BACKEND_RUN),
    poll: vi.fn(async () => BACKEND_RUN),
    runLocal: vi.fn(async () => BACKEND_RUN),
    format: vi.fn(async (p: { lang: string; code: string }) => p.code),
    loginCaptureStatus: vi.fn(async () => ({ active: false, expiresAt: null })),
    ...over,
  };
}

/** The runner setting at its default and every problem's custom cases dropped. */
export function resetConsoleTwin(...problemIds: string[]): void {
  useRunnerSettingStore.setState({ runner: DEFAULT_RUNNER });
  for (const id of problemIds) useConsoleCasesStore.getState().reset(ProblemCatalog.getByIdOrThrow(id));
}

export const tap = (id: string) => userEvent.click(screen.getByTestId(id));
export const text = (id: string) => screen.getByTestId(id).textContent;
export const exists = (id: string) => screen.queryByTestId(id) !== null;
export const verdict = () => text('ConsoleResultVerdict');
