// features/coding-agent/tests/flow/dispatchRunsStore.flow.test.ts
//
// Flow-tier test (store driven through the fixture gateway) for `useDispatchRunsStore` — covers the three branches the
// section's UI depends on: the happy-path list load via the fixture
// gateway, the running fixture's per-call line growth, and the failure
// branch where a rejecting gateway leaves the previous runs in place while
// flipping `status` to `'error'` with the thrown message attached.
import {
  FIXTURE_DISPATCH_RUN_IDS,
  type DispatchRunMeta,
} from '../../src/domain';
import { beforeEach, describe, expect, it } from 'vitest';

import {
  createFixtureDispatchRunsGateway,
  DispatchRunsGatewayError,
  type DispatchRunsGateway,
} from '../../src/data/dispatchRunsGateway';
import { useDispatchRunsStore } from '../../src/state/dispatchRunsStore';

describe('dispatchRunsStore', () => {
  beforeEach(() => {
    useDispatchRunsStore.setState({
      runs: [],
      details: {},
      status: 'idle',
      error: undefined,
    });
  });

  it('given the fixture gateway, when loadRuns resolves, then runs are filled and status becomes ready', async () => {
    // Given the fixture gateway
    const gateway = createFixtureDispatchRunsGateway();

    // When loadRuns resolves
    await useDispatchRunsStore.getState().loadRuns(gateway);

    // Then runs are filled and status becomes ready
    const state = useDispatchRunsStore.getState();
    expect(state.runs.length).toBeGreaterThan(0);
    expect(state.status).toBe('ready');
  });

  it('given the running fixture, when loadRun runs twice, then detail lines grow by 2', async () => {
    // Given the fixture gateway and the running fixture
    const gateway = createFixtureDispatchRunsGateway();
    const runId = FIXTURE_DISPATCH_RUN_IDS.running;

    // When loadRun runs twice
    await useDispatchRunsStore.getState().loadRun(gateway, runId);
    const firstLength =
      useDispatchRunsStore.getState().details[runId]?.lines.length ?? 0;
    expect(firstLength).toBeGreaterThan(0);

    await useDispatchRunsStore.getState().loadRun(gateway, runId);
    const secondLength =
      useDispatchRunsStore.getState().details[runId]?.lines.length ?? 0;

    // Then detail lines grow by 2
    expect(secondLength - firstLength).toBe(2);
  });

  it('given a gateway whose listRuns rejects, when loadRuns is invoked, then status becomes error and previous runs are kept', async () => {
    // Given a previously successful list and a gateway whose listRuns rejects
    const previousRuns: DispatchRunMeta[] = [
      {
        runId: 'kept-run',
        prompt: 'previous',
        agent: null,
        caller: null,
        claudeSessionId: null,
        model: null,
        startedAt: '2026-01-01T00:00:00.000Z',
        finishedAt: null,
        status: 'running',
        ok: null,
        tokens: null,
        cost: null,
        durationMs: null,
      },
    ];
    useDispatchRunsStore.setState({ runs: previousRuns, status: 'ready' });

    const rejectingGateway: DispatchRunsGateway = {
      listRuns: async () => {
        throw new DispatchRunsGatewayError('listRuns: schema_failed', 0);
      },
      getRun: async () => {
        throw new Error('unused');
      },
    };

    // When loadRuns is invoked
    await useDispatchRunsStore.getState().loadRuns(rejectingGateway);

    // Then status becomes error and previous runs are kept
    const state = useDispatchRunsStore.getState();
    expect(state.status).toBe('error');
    expect(state.runs).toBe(previousRuns);
    expect(state.error).toBeDefined();
  });
});