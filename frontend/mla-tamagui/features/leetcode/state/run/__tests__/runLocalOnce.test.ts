import { describe, expect, it } from 'vitest';

import type { ErrorCheckpoint } from '../../../domain/editor/errorJump';
import type { CodeRunnerPort, RunCodeParams } from '../../../domain/ports';
import { createFakeCodeRunner, passingRun } from '../../../testing/fakeCodeRunner';
import { runLocalOnce } from '../runLocalOnce';

const GIVEN: RunCodeParams = { lang: 'python3', code: 'screen text', methodName: 'f', testCases: [] };
const checkpointOf = (text: string): ErrorCheckpoint => ({ text, revision: 1 as never, instance: 'i' });

describe('runLocalOnce', () => {
  it('Given an editor that answers a checkpoint, When it runs, Then the runner receives the checkpoint text, not the screen text', async () => {
    const runner = createFakeCodeRunner();

    const outcome = await runLocalOnce({ runner, editor: { checkpoint: async () => checkpointOf('editor text') }, given: GIVEN, isCurrent: () => true, problemId: 'p', mode: 'tests' });

    expect(runner.calls[0]?.code).toBe('editor text');
    expect(outcome.kind).toBe('answered');
  });

  it('Given no checkpoint in time, When it runs, Then it runs on the screen text', async () => {
    const runner = createFakeCodeRunner();

    await runLocalOnce({ runner, editor: { checkpoint: async () => null }, given: GIVEN, isCurrent: () => true, problemId: 'p', mode: 'tests' });

    expect(runner.calls[0]?.code).toBe('screen text');
  });

  it('Given the code moves before the answer, When it lands, Then the outcome is discarded', async () => {
    let current = true;
    const runner: CodeRunnerPort = {
      run: async () => {
        current = false;
        return passingRun();
      },
    };

    const outcome = await runLocalOnce({ runner, editor: null, given: GIVEN, isCurrent: () => current, problemId: 'p', mode: 'tests' });

    expect(outcome.kind).toBe('discarded');
  });

  it('Given the runner throws, When it runs, Then the outcome is failed with the error', async () => {
    const boom = new Error('boom');
    const runner: CodeRunnerPort = {
      run: async () => {
        throw boom;
      },
    };

    const outcome = await runLocalOnce({ runner, editor: null, given: GIVEN, isCurrent: () => true, problemId: 'p', mode: 'tests' });

    expect(outcome).toMatchObject({ kind: 'failed', error: boom });
  });
});
