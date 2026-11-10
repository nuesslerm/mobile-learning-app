// features/coding-agent/src/fixtures/__tests__/fixtures.test.ts
//
// Smoke tests for the deterministic fixtures: every `FIXTURE_THREADS` entry
// round-trips through `OrchestrationThreadSchema` (the schema that the wire
// layer enforces), the `fx-tool-calls` fixture collapses to a single
// work-log row with a `work-toggle` carrying the hidden count, and the
// `fx-approval` fixture surfaces two pending approvals. Every test is
// Given/When/Then.

import {
  OrchestrationThreadSchema,
  type OrchestrationThread,
} from '@app/remote-chat-agent-contracts/orchestration';
import { describe, expect, it } from 'vitest';

import { buildThreadFeed } from '../../domain/threadFeed';
import { derivePendingApprovals } from '../../domain/pendingRequests';
import { FIXTURE_THREADS, type FixtureId } from '../threads';

describe('FIXTURE_THREADS', () => {
  it('given every fixture thread, when parsed with OrchestrationThreadSchema, then it round-trips', () => {
    // Given every fixture id
    const fixtureIds = Object.keys(FIXTURE_THREADS) as FixtureId[];
    // When each fixture is re-parsed by the wire schema
    const roundTripped: Record<FixtureId, OrchestrationThread> = Object.fromEntries(
      fixtureIds.map((id) => [id, OrchestrationThreadSchema.parse(FIXTURE_THREADS[id])])
    ) as Record<FixtureId, OrchestrationThread>;
    // Then every fixture round-trips equal to its source
    for (const id of fixtureIds) {
      expect(roundTripped[id]).toEqual(FIXTURE_THREADS[id]);
    }
  });
});

describe('fx-tool-calls feed', () => {
  it('given fx-tool-calls, when built with nothing expanded, then a work-toggle with hiddenCount 7 is emitted', () => {
    // Given the fx-tool-calls fixture (8 tool activities in a single turn)
    const thread = FIXTURE_THREADS['fx-tool-calls'];
    // When the thread feed is built with no work-log groups expanded
    const feed = buildThreadFeed(thread);
    // Then a work-toggle is present carrying the hidden count
    const toggle = feed.find((entry) => entry.kind === 'work-toggle');
    expect(toggle).toMatchObject({
      kind: 'work-toggle',
      hiddenCount: 7,
    });
  });
});

describe('fx-approval pending requests', () => {
  it('given fx-approval, when derived, then two pending approvals are returned', () => {
    // Given the fx-approval fixture (two request.opened activities, no resolves)
    const thread = FIXTURE_THREADS['fx-approval'];
    // When pending approvals are derived
    const pending = derivePendingApprovals(thread);
    // Then both approvals are still open
    expect(pending).toHaveLength(2);
  });
});
