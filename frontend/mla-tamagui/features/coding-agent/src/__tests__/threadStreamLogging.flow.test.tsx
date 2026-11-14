// @vitest-environment jsdom
//
// features/coding-agent/src/__tests__/threadStreamLogging.flow.test.tsx
//
// What an open thread leaves in BetterStack when its run fails on the phone: events that had no snapshot to land on,
// the snapshot that then arrived, and each change of the run's state with the provider's error. The gateway is the
// data-level double and the subscription hook runs for real.
import { TurnId } from '@app/remote-chat-agent-contracts/orchestration';
import { act } from '@lib/flow-test-harness';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { useThreadSubscription } from '../state/useThreadSubscription';
import { useGatewayStore } from '../state/gatewayStore';
import { useThreadDetailStore } from '../state/threadDetailStore';
import { FakeGateway } from '../state/__tests__/fakeGateway';
import { captureLogs } from './captureLogs';
import { app } from './flowApp';
import {
  THREAD_ID,
  makeThread,
  messageSentEvent,
  sessionSetEvent,
} from './threadFixtures';

describe('thread stream logs', () => {
  let logs: ReturnType<typeof captureLogs>;

  beforeEach(() => {
    logs = captureLogs();
    useThreadDetailStore.setState({
      threads: {},
      noSnapshotWarned: {},
      expandedGroupIds: new Set(),
    });
    useGatewayStore.setState({ gateway: null, transport: null, status: 'idle' });
  });

  afterEach(() => logs.stop());

  it('given an open thread whose first events come before its snapshot, when the run then starts and fails, then the lines name the dropped events once, the snapshot, and the failed run with the provider message', () => {
    // Given a gateway streaming to a mounted thread subscription
    const gateway = new FakeGateway();
    useGatewayStore.getState().setGateway(gateway, 'fixture');
    app.renderHook(() => useThreadSubscription(THREAD_ID));

    // When two events beat the snapshot, then the snapshot lands, the run starts and the session fails
    act(() => {
      gateway.push(THREAD_ID, { kind: 'event', event: messageSentEvent(4) });
      gateway.push(THREAD_ID, { kind: 'event', event: messageSentEvent(5) });
      gateway.push(THREAD_ID, { kind: 'snapshot', snapshot: makeThread() });
      gateway.push(THREAD_ID, {
        kind: 'event',
        event: sessionSetEvent(6, {
          cli: 'opencode',
          status: 'running',
          activeTurnId: TurnId.parse('turn-1'),
        }),
      });
      gateway.push(THREAD_ID, {
        kind: 'event',
        event: sessionSetEvent(7, {
          cli: 'opencode',
          status: 'error',
          lastError: 'provider down',
        }),
      });
    });

    // Then one warning names the first dropped event, not one per event
    const dropped = logs.named('coding-agent.thread.event_dropped');
    expect(dropped).toHaveLength(1);
    expect(dropped[0]).toMatchObject({
      level: 'warn',
      fields: { threadId: THREAD_ID, type: 'thread.message-sent', sequence: 4, reason: 'no-snapshot' },
    });

    // And the snapshot is on record with its counts
    expect(logs.named('coding-agent.thread.snapshot')[0]).toMatchObject({
      level: 'info',
      fields: { threadId: THREAD_ID, messages: 0, activities: 0, plans: 0 },
    });

    // And the run's two transitions are on record, the failure as a warning carrying the provider's message
    const states = logs.named('coding-agent.turn.state');
    expect(states).toHaveLength(2);
    expect(states[0]).toMatchObject({
      level: 'info',
      fields: { threadId: THREAD_ID, turnId: 'turn-1', to: 'running', sessionStatus: 'running' },
    });
    expect(states[1]).toMatchObject({
      level: 'warn',
      fields: {
        turnId: 'turn-1',
        from: 'running',
        to: 'error',
        sessionStatus: 'error',
        lastError: 'provider down',
      },
    });
  });
});
