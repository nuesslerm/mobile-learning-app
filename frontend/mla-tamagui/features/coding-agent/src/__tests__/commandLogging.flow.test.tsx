// @vitest-environment jsdom
//
// features/coding-agent/src/__tests__/commandLogging.flow.test.tsx
//
// What a prompt the server refuses leaves in BetterStack: turn.start when it leaves, dispatch_failed with why and that
// it was queued. The prompt text is user content and never appears. The gateway is the data-level double, so the whole
// hook runs for real.
import type {
  ClientOrchestrationCommand,
  OrchestrationCommandReceipt,
} from '@app/remote-chat-agent-contracts/orchestration';
import { act } from '@lib/flow-test-harness';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { useThreadCommands } from '../integration/useThreadCommands';
import { useComposerStore } from '../state/composerStore';
import { useGatewayStore } from '../state/gatewayStore';
import { useOutboxStore } from '../state/outboxStore';
import { useThreadDetailStore } from '../state/threadDetailStore';
import { FakeGateway } from '../state/__tests__/fakeGateway';
import { captureLogs } from './captureLogs';
import { app } from './flowApp';
import { THREAD_ID } from './threadFixtures';

const PROMPT = 'PRIVATE prompt text the user typed';

const rejecting = Object.assign(new FakeGateway(), {
  dispatch: async (
    command: ClientOrchestrationCommand
  ): Promise<OrchestrationCommandReceipt> => ({
    commandId: command.commandId,
    status: 'rejected',
    events: [],
    error: { code: 'not_allowed', message: 'not allowed' },
  }),
});

describe('command dispatch logs', () => {
  let logs: ReturnType<typeof captureLogs>;

  beforeEach(() => {
    logs = captureLogs();
    useGatewayStore.setState({ gateway: null, transport: null, status: 'idle' });
    useThreadDetailStore.setState({ threads: {}, expandedGroupIds: new Set() });
    useOutboxStore.setState({ items: [], hydrated: false });
    useComposerStore.setState({
      drafts: {},
      runtimeMode: {},
      interactionMode: {},
      modelSelection: {},
      queued: {},
    });
  });

  afterEach(() => logs.stop());

  it('given a gateway that rejects commands, when a prompt is sent, then turn.start and dispatch_failed share the commandId, the failure says rejected with its code and that it was queued, and no line carries the text', async () => {
    // Given a hook over a gateway answering every command ok:false with a code
    useGatewayStore.getState().setGateway(rejecting, 'fixture');
    const { result } = app.renderHook(() => useThreadCommands(THREAD_ID));

    // When the user sends a prompt
    await act(async () => {
      await result.current.startTurn(PROMPT);
    });

    // Then turn.start carries sizes and modes only, and dispatch_failed names the same command
    const start = logs.named('coding-agent.turn.start')[0];
    expect(start?.fields).toMatchObject({
      threadId: THREAD_ID,
      textLength: PROMPT.length,
      steer: false,
      bootstrap: true,
      cli: 'opencode',
    });
    const failed = logs.named('coding-agent.commands.dispatch_failed');
    expect(failed).toHaveLength(1);
    expect(failed[0]).toMatchObject({
      level: 'warn',
      fields: {
        commandId: start?.fields.commandId,
        type: 'thread.turn.start',
        reason: 'rejected',
        code: 'not_allowed',
        gatewayStatus: 'open',
        queued: true,
        ms: expect.any(Number),
      },
    });
    expect(logs.named('coding-agent.commands.dispatched')).toHaveLength(0);
    expect(useOutboxStore.getState().items).toHaveLength(1);
    expect(logs.dump()).not.toContain('PRIVATE');
  });
});
