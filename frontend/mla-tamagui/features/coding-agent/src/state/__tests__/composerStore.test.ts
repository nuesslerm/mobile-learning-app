// features/coding-agent/src/state/__tests__/composerStore.test.ts
//
// State test for `useComposerStore` selector stability. Zustand 5 drives
// re-renders through `useSyncExternalStore`: a selector that returns a NEW
// reference on every call (a `?? []` fallback) re-renders forever, so the
// empty fallback must be the module-level `EMPTY_QUEUE` constant.
import { ThreadId } from '@app/remote-chat-agent-contracts/orchestration';
import { beforeEach, describe, expect, it } from 'vitest';

import { EMPTY_QUEUE, useComposerStore } from '../composerStore';

const THREAD_ID = ThreadId.parse('thread-1');

describe('composerStore queued selector stability', () => {
  beforeEach(() => {
    useComposerStore.setState({ queued: {} });
  });

  it('given a thread with no queued messages, when the queued selector runs twice, then both results are the same reference (Object.is)', () => {
    // Given a thread with no queued messages
    type Store = ReturnType<typeof useComposerStore.getState>;
    const selectQueued = (state: Store) => state.queued[THREAD_ID] ?? EMPTY_QUEUE;

    // When the queued selector runs twice
    const first = selectQueued(useComposerStore.getState());
    const second = selectQueued(useComposerStore.getState());

    // Then both results are the same reference
    expect(Object.is(first, second)).toBe(true);
  });
});