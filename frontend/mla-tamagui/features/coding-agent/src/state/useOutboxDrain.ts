import { useEffect } from 'react';
import { Logger } from '@lib/logging';
import {
  nextDue,
  type ClientOrchestrationCommand,
} from '../domain';

import { useGatewayStore } from '../state/gatewayStore';
import { useOutboxStore } from '../state/outboxStore';

export function useOutboxDrain(): void {
  const gateway = useGatewayStore((state) => state.gateway);

  useEffect(() => {
    if (gateway === null) return;

    let cancelled = false;
    let draining = false;

    const drain = async (): Promise<void> => {
      if (cancelled || draining || gateway.status() !== 'open') return;
      draining = true;
      try {
        while (!cancelled && gateway.status() === 'open') {
          const item = nextDue(useOutboxStore.getState().items, Date.now());
          if (item === undefined) return;

          try {
            const command = item.command as ClientOrchestrationCommand;
            const startedAt = Date.now();
            const receipt = await gateway.dispatch(command);
            if (receipt.status === 'accepted') {
              Logger.info('coding-agent.outbox.drained', {
                id: item.id,
                threadId: item.threadId,
                type: command.type,
                attempts: item.attempts,
                ms: Date.now() - startedAt,
              });
              useOutboxStore.getState().remove(item.id);
              continue;
            }
            useOutboxStore.getState().markAttempt(item.id);
            Logger.withMetadata({
              id: item.id,
              threadId: item.threadId,
              type: command.type,
              attempts: item.attempts,
              code: receipt.error?.code,
            }).warn('coding-agent.outbox.dispatch_rejected');
            return;
          } catch (cause) {
            useOutboxStore.getState().markAttempt(item.id);
            Logger.withError(cause).withMetadata({
              id: item.id,
              threadId: item.threadId,
              attempts: item.attempts,
            }).warn('coding-agent.outbox.dispatch_failed');
            return;
          }
        }
      } finally {
        draining = false;
      }
    };

    const trigger = (): void => {
      void drain();
    };

    const detachStatus = gateway.onStatus((status) => {
      if (status === 'open') trigger();
    });
    const unsubscribe = useOutboxStore.subscribe(() => {
      if (gateway.status() === 'open') trigger();
    });

    const outbox = useOutboxStore.getState();
    if (outbox.hydrated || outbox.items.length > 0) {
      trigger();
    } else {
      void outbox.hydrate().then(trigger).catch((cause: unknown) => {
        Logger.withError(cause).warn('coding-agent.outbox.hydrate_failed');
      });
    }

    return () => {
      cancelled = true;
      detachStatus();
      unsubscribe();
    };
  }, [gateway]);
}
