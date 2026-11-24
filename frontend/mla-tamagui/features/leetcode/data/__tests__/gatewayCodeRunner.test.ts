import { describe, expect, it, vi } from 'vitest';

import type { RunResponse } from '../../domain';
import type { LeetCodeGatewayPort } from '../../domain/ports';
import { passingRun } from '../../testing';
import { createGatewayCodeRunner } from '../gatewayCodeRunner';

describe('the gateway code runner', () => {
  it('given a gateway, when it runs a solution, then it asks the gateway to run it locally with the same request and returns the gateway\'s answer', async () => {
    // Given a gateway that answers a run with a known response
    const answer: RunResponse = passingRun(2);
    const runLocal = vi.fn(async () => answer);
    const runner = createGatewayCodeRunner({ runLocal } as unknown as LeetCodeGatewayPort);
    const request = { lang: 'python3', code: 'x = 1', methodName: 'twoSum', testCases: [] };

    // When the runner runs the solution
    const result = await runner.run(request);

    // Then the gateway was asked once with that request, and its answer comes back as it was
    expect(runLocal).toHaveBeenCalledExactlyOnceWith(request);
    expect(result).toBe(answer);
  });
});
