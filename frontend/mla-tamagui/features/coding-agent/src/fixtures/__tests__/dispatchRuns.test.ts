// features/coding-agent/src/fixtures/__tests__/dispatchRuns.test.ts
//
// Smoke tests for the deterministic `FIXTURE_DISPATCH_RUNS` fixtures:
// every fixture satisfies `DispatchRunDetail` (parsed at module load), and
// the running fixture carries the expected `running` status. Given/When/Then.

import { describe, expect, it } from 'vitest';

import {
  DispatchRunDetail,
  type DispatchRunDetail as DispatchRunDetailType,
} from '../../domain/dispatchRuns';
import { FIXTURE_DISPATCH_RUNS, FIXTURE_DISPATCH_RUN_IDS } from '../dispatchRuns';

describe('FIXTURE_DISPATCH_RUNS', () => {
  it('given both fixtures, when re-parsed with DispatchRunDetail.parse, then they round-trip', () => {
    // Given the two fixtures as parsed at module load
    const { finished, running } = FIXTURE_DISPATCH_RUNS;
    // When each is re-parsed through the wire schema
    const finishedRoundTripped: DispatchRunDetailType = DispatchRunDetail.parse(finished);
    const runningRoundTripped: DispatchRunDetailType = DispatchRunDetail.parse(running);
    // Then both round-trip equal to their source
    expect(finishedRoundTripped).toEqual(finished);
    expect(runningRoundTripped).toEqual(running);
  });

  it('given the running fixture, when inspected, then its status is running and ids match', () => {
    // Given the running fixture and its declared ids
    const running = FIXTURE_DISPATCH_RUNS.running;
    // Then it carries the expected status and stable id
    expect(running.status).toBe('running');
    expect(running.runId).toBe(FIXTURE_DISPATCH_RUN_IDS.running);
    expect(FIXTURE_DISPATCH_RUN_IDS.running).toBe('fx-run-running');
    expect(FIXTURE_DISPATCH_RUN_IDS.finished).toBe('fx-run-finished');
  });
});