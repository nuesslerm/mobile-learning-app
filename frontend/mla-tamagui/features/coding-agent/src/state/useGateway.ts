// Owns the singleton `OrchestrationGateway` while a screen is mounted. The orchestration port opens the gateway
// (the live one resolves its config and opens a WebSocket), it is pushed into `useGatewayStore` so the state
// layer can subscribe to it, and `connect()` is kicked. The current `OrchestrationGatewayStatus` is exposed so a
// screen can render a spinner or an error banner without reaching into the store directly.
import { useEffect } from 'react';
import { Logger } from '@lib/logging';

import type { OrchestrationGatewayStatus } from '../domain/ports';
import { useGatewayStore } from './gatewayStore';
import { useCodingAgentPorts } from './ports';

export function useGateway(): OrchestrationGatewayStatus {
  const status = useGatewayStore((s) => s.status);
  const { orchestration } = useCodingAgentPorts();

  useEffect(() => {
    let cancelled = false;
    const { setGateway, connect, disconnect } = useGatewayStore.getState();

    void (async () => {
      try {
        const gateway = await orchestration.open();
        if (cancelled) {
          gateway.close();
          return;
        }
        Logger.info('coding-agent.gateway.created', {});
        setGateway(gateway, orchestration.transport);
        await connect();
      } catch (cause) {
        Logger.withError(cause).warn('coding-agent.gateway.create_failed');
      }
    })();

    return () => {
      cancelled = true;
      disconnect();
    };
  }, [orchestration]);

  return status;
}
