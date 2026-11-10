// @vitest-environment jsdom
//
// features/coding-agent/src/__tests__/threadFlow.flow.test.tsx
//
// Flow test for the coding-agent feature. Boots the four integration hooks
// over the in-memory `fixture` transport for `fx-thread-fx-approval`,
// asserts the feed eventually surfaces the fixture entries, and asserts that
// `respondApproval` dispatches `thread.approval.respond` to the fixture
// gateway. Per the test-tiers contract, flow tests boot the composition
// root (here `useGateway` + `useThreadSubscription` + `useThreadFeed` +
// `useThreadCommands`) with the gateway replaced by the in-memory fixture,
// and assert observable output (the feed, the gateway's recorded commands).
import { ThreadId } from '@app/remote-chat-agent-contracts/orchestration';
import { act, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { app } from './flowApp';
import {
  createFixtureGateway,
  type FixtureGateway,
} from '../data/fixtureGateway';
import { useGateway } from '../integration/useGateway';
import { useThreadCommands } from '../integration/useThreadCommands';
import { useThreadFeed } from '../integration/useThreadFeed';
import { useThreadSubscription } from '../integration/useThreadSubscription';
import { useGatewayStore } from '../state/gatewayStore';
import { useOutboxStore } from '../state/outboxStore';
import { useThreadDetailStore } from '../state/threadDetailStore';
import { useThreadShellStore } from '../state/threadShellStore';

const THREAD_ID = ThreadId.parse('fx-thread-fx-approval');

function useThreadFlow(threadId: ThreadId) {
  useGateway();
  useThreadSubscription(threadId);
  const feed = useThreadFeed(threadId);
  const commands = useThreadCommands(threadId);
  return { feed, commands };
}

describe('threadFlow (fixture transport)', () => {
  beforeEach(() => {
    useThreadDetailStore.setState({ threads: {}, expandedGroupIds: new Set() });
    useGatewayStore.setState({
      gateway: null,
      transport: null,
      status: 'idle',
    });
    useOutboxStore.setState({ items: [], hydrated: false });
    useThreadShellStore.setState({ threads: [] });
  });

  afterEach(() => {
    act(() => useGatewayStore.getState().disconnect());
  });

  it('given the fixture transport for fx-thread-fx-approval, when the fixture snapshot lands, then the feed has the fixture entries with a pending approval and respondApproval dispatches thread.approval.respond', async () => {
    // Given a renderHook composition of useGateway() + useThreadSubscription + useThreadFeed + useThreadCommands for fx-thread-fx-approval, with the gateway replaced by the fixture override
    const { result } = app.renderHook(() => useThreadFlow(THREAD_ID), {
      overrides: { 'coding-agent': { gateway: createFixtureGateway() } },
    });

    // When the fixture gateway creates and the snapshot is applied to the thread detail store
    await waitFor(() => {
      expect(result.current.feed.feed.length).toBeGreaterThan(0);
    });

    // Then the feed has at least one pending approval and the snapshot-derived thread is the one under test
    expect(result.current.feed.pendingApprovals.length).toBeGreaterThanOrEqual(
      1
    );
    expect(useThreadDetailStore.getState().threads[THREAD_ID]).toBeDefined();

    // When respondApproval is called with the first pending approval request
    const request = result.current.feed.pendingApprovals[0];
    await act(() =>
      result.current.commands.respondApproval(request.requestId, 'accept')
    );

    // Then the fixture gateway recorded the thread.approval.respond dispatch
    const gateway = useGatewayStore.getState().gateway as FixtureGateway | null;
    expect(gateway).not.toBeNull();
    const recorded = gateway!.recordedCommands.find(
      (cmd) => cmd.type === 'thread.approval.respond'
    );
    expect(recorded).toMatchObject({
      type: 'thread.approval.respond',
      threadId: THREAD_ID,
      requestId: request.requestId,
      decision: 'accept',
    });
  });

  it('creates a fresh recordedCommands list per createFixtureGateway call', () => {
    // Given two independent fixture gateways
    const a = createFixtureGateway();
    const b = createFixtureGateway();
    // When we check that they each carry an empty list initially
    expect(a.recordedCommands).toHaveLength(0);
    expect(b.recordedCommands).toHaveLength(0);
    // Then mutating one does not leak into the other
    expect(a.recordedCommands).not.toBe(b.recordedCommands);
  });
});
