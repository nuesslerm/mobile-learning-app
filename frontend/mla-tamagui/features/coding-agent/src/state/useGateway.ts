// Owns the singleton `OrchestrationGateway` while a screen is mounted.
// An injected gateway override wins; otherwise the real WS gateway is
// created from the runtime config, pushed into `useGatewayStore` so the
// integration layer can subscribe to it, and `connect()` is kicked. The
// current `OrchestrationGatewayStatus` is exposed so a screen can render
// a spinner or an error banner without reaching into the store directly.
import { useEffect } from 'react';
import { useDependencyOverrides } from '@lib/app-di';
import { Logger } from '@lib/logging';

import {
  resolveCodingAgentConfig,
  type CodingAgentConfig,
} from '../data/codingAgentConfig';
import type {
  OrchestrationGateway,
  OrchestrationGatewayStatus,
} from '../data/orchestrationGateway';
import { createWsGateway } from '../data/wsGateway';
import { useGatewayStore } from '../state/gatewayStore';

export function useGateway(): OrchestrationGatewayStatus {
  const status = useGatewayStore((s) => s.status);
  const { gateway: overriddenGateway } =
    useDependencyOverrides<{ gateway: OrchestrationGateway }>('coding-agent');

  useEffect(() => {
    let cancelled = false;
    const { setGateway, connect, disconnect } = useGatewayStore.getState();

    const createGateway = async (): Promise<OrchestrationGateway> => {
      if (overriddenGateway !== undefined) return overriddenGateway;
      const config: CodingAgentConfig = await resolveCodingAgentConfig();
      return createWsGateway(config);
    };

    void (async () => {
      try {
        const gateway = await createGateway();
        if (cancelled) {
          gateway.close();
          return;
        }
        Logger.info('coding-agent.gateway.created', {});
        setGateway(gateway, overriddenGateway !== undefined ? 'fixture' : 'ws');
        await connect();
      } catch (cause) {
        Logger.withError(cause).warn('coding-agent.gateway.create_failed');
      }
    })();

    return () => {
      cancelled = true;
      disconnect();
    };
  }, [overriddenGateway]);

  return status;
}