// features/coding-agent/src/state/useThreadSubscription.ts
//
// Integration hook that wires a single thread subscription to the active
// `OrchestrationGateway`: forwards every `OrchestrationSubscribeThreadItem`
// into `useThreadDetailStore.applyItem` (synchronized is a no-op there,
// snapshot replaces the cached thread, event runs the pure reducer), and
// resubscribes on reconnect from `lastSequence + 1` so a dropped socket
// doesn't leave the screen stuck on a stale snapshot. Returns the current
// `OrchestrationGatewayStatus` so a screen can render a connecting banner
// or hide live-follow controls while the WS is down.
import { useEffect, useRef } from 'react';
import { Logger } from '@lib/logging';
import type { ThreadId } from '@app/remote-chat-agent-contracts/orchestration';

import type {
  OrchestrationGatewayStatus,
  OrchestrationThreadListener,
} from '../data/orchestrationGateway';
import { useGatewayStore } from '../state/gatewayStore';
import {
  threadSessionFields,
  useThreadDetailStore,
} from '../state/threadDetailStore';

export function useThreadSubscription(threadId: ThreadId): OrchestrationGatewayStatus {
  const gateway = useGatewayStore((s) => s.gateway);
  const status = useGatewayStore((s) => s.status);
  const lastSequenceRef = useRef<number | undefined>(undefined);
  const detachRef = useRef<(() => void) | null>(null);
  const connectedRef = useRef<boolean>(false);

  useEffect(() => {
    if (gateway === null) {
      lastSequenceRef.current = undefined;
      connectedRef.current = false;
      return;
    }

    const apply: OrchestrationThreadListener = (item) => {
      if (
        item.kind === 'event' &&
        typeof item.event.sequence === 'number'
      ) {
        const prev = lastSequenceRef.current;
        lastSequenceRef.current =
          prev === undefined
            ? item.event.sequence
            : Math.max(prev, item.event.sequence);
      }
      useThreadDetailStore.getState().applyItem(threadId, item);
      if (item.kind === 'snapshot') {
        // Logged here, not in the store: a thread started offline gets a local snapshot that never crossed the wire.
        Logger.info('coding-agent.thread.snapshot', {
          threadId,
          messages: item.snapshot.messages.length,
          activities: item.snapshot.activities.length,
          plans: item.snapshot.proposedPlans.length,
          ...threadSessionFields(item.snapshot),
        });
      }
    };

    const subscribe = (afterSequence?: number): void => {
      detachRef.current?.();
      detachRef.current = gateway.subscribeThread(
        threadId,
        apply,
        afterSequence,
      );
    };
    subscribe(
      lastSequenceRef.current === undefined
        ? undefined
        : lastSequenceRef.current + 1,
    );

    const offStatus = gateway.onStatus((next) => {
      const wasOpen = connectedRef.current;
      connectedRef.current = next === 'open';
      if (next === 'open' && !wasOpen) {
        subscribe(
          lastSequenceRef.current === undefined
            ? undefined
            : lastSequenceRef.current + 1,
        );
      }
    });

    return () => {
      offStatus();
      detachRef.current?.();
      detachRef.current = null;
      connectedRef.current = false;
    };
  }, [gateway, threadId]);

  return status;
}