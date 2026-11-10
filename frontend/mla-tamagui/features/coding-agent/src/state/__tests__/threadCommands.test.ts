// @vitest-environment jsdom
// features/coding-agent/src/state/__tests__/threadCommands.test.ts
//
// State test for `useThreadCommands.setModelSelection` with an injected fake
// gateway, in the Given/When/Then style the other state tests use. Picking a
// model must both update the composer store (as the sheet did before) and
// dispatch a `thread.meta.update` command carrying that `modelSelection` on
// the wire.
import { act, renderHook } from '@testing-library/react';
import {
  ThreadId,
  type ClientOrchestrationCommand,
  type ModelSelection,
} from '@app/remote-chat-agent-contracts/orchestration';
import { beforeEach, describe, expect, it } from 'vitest';

import { useThreadCommands } from '../../integration/useThreadCommands';
import { useComposerStore } from '../composerStore';
import { useGatewayStore } from '../gatewayStore';
import { FakeGateway } from './fakeGateway';

const THREAD_ID = ThreadId.parse('thread-1');

describe('useThreadCommands.setModelSelection', () => {
  beforeEach(() => {
    useComposerStore.setState({
      drafts: {},
      runtimeMode: {},
      interactionMode: {},
      modelSelection: {},
      queued: {},
    });
    useGatewayStore.setState({ gateway: null, transport: null, status: 'idle' });
  });

  it('given a fake gateway recording dispatches, when setModelSelection runs, then one thread.meta.update with that modelSelection was dispatched and the composer store holds the selection', async () => {
    // Given a fake gateway recording dispatches
    const fake = new FakeGateway();
    useGatewayStore.getState().setGateway(fake, 'fixture');

    const { result } = renderHook(() => useThreadCommands(THREAD_ID));

    // When setModelSelection runs
    const selection: ModelSelection = { cli: 'opencode', modelID: 'gpt-5.4' };
    await act(async () => {
      await result.current.setModelSelection(selection);
    });

    // Then one thread.meta.update with that modelSelection was dispatched
    expect(fake.dispatched).toHaveLength(1);
    const dispatched = fake.dispatched[0] as ClientOrchestrationCommand & {
      type: 'thread.meta.update';
    };
    expect(dispatched.type).toBe('thread.meta.update');
    expect(dispatched.threadId).toBe(THREAD_ID);
    expect(dispatched.modelSelection).toEqual(selection);

    // And the composer store holds the selection
    expect(useComposerStore.getState().modelSelection[THREAD_ID]).toEqual(
      selection,
    );
  });
});
