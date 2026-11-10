import { act, screen, waitFor } from '@lib/flow-test-harness';
import { describe, expect, it, vi } from 'vitest';

import { app } from '../../../../flow/app';
import type {
  LeetCodeGatewayPort,
  RunResponse,
} from '../../../../../features/leetcode';

const MINIMAL_RUN: RunResponse = {
  source: 'backend-run',
  kind: 'test',
  state: 'SUCCESS',
  requestId: '',
  run_success: true,
  status_msg: 'Accepted',
  total_correct: 0,
  total_testcases: 0,
};

// The gateway port is resolved asynchronously (macrotask) so the screen's
// async effects settle inside act, not in the microtask queue that fires the
// moment app.render resumes.
function resolveNextTick<T>(value: T): Promise<T> {
  return new Promise((resolve) => setTimeout(() => resolve(value), 0));
}

function fakeGateway(): LeetCodeGatewayPort {
  return {
    userStatus: vi.fn(() =>
      resolveNextTick({ isSignedIn: true, username: 'fixture-user' })
    ),
    test: vi.fn(() => resolveNextTick(MINIMAL_RUN)),
    submit: vi.fn(() => resolveNextTick(MINIMAL_RUN)),
    poll: vi.fn(() => resolveNextTick(MINIMAL_RUN)),
    runLocal: vi.fn(() => resolveNextTick(MINIMAL_RUN)),
    format: vi.fn(async (p: { lang: string; code: string }) => p.code),
    loginCaptureStatus: vi.fn(() =>
      resolveNextTick({ active: false, expiresAt: null })
    ),
  };
}

describe('LoginScreen flow (gateway status)', () => {
  it('given a fake gateway signed in as fixture-user, when the login route renders, then the status shows that user', async () => {
    const gateway = fakeGateway();
    await app.render('/leetcode/login', { overrides: { leetcode: { gateway } } });
    await act(async () => {
      await new Promise<void>((resolve) => setTimeout(resolve, 0));
    });

    await waitFor(() => {
      expect(screen.getByTestId('LoginScreenMessage').textContent).toBe(
        'Signed in as fixture-user'
      );
    });
    expect(gateway.userStatus).toHaveBeenCalled();
    expect(gateway.loginCaptureStatus).toHaveBeenCalled();
  });
});