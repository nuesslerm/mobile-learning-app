// features/coding-agent/src/state/gatewayStore.ts
//
// Zustand store that owns the active `OrchestrationGateway` for the
// coding-agent feature. The UI binds once at app start to the appropriate
// transport (`ws` against the real orchestration server, `fixture` against
// `createFixtureGateway` for offline / storybook runs), then calls
// `connect()` / `disconnect()` around its lifetime. Every state mutation
// goes through one of the logged actions so we can correlate user-visible
// state with the WS-RPC traffic it produced.
import { Logger } from '@lib/logging';
import { create } from 'zustand';

import type {
  OrchestrationGateway,
  OrchestrationGatewayStatus,
} from '../data/orchestrationGateway';

export type CodingAgentTransport = 'ws' | 'fixture';

type GatewayState = {
  gateway: OrchestrationGateway | null;
  transport: CodingAgentTransport | null;
  status: OrchestrationGatewayStatus;
  setGateway: (
    gateway: OrchestrationGateway,
    transport: CodingAgentTransport,
  ) => void;
  connect: () => Promise<void>;
  disconnect: () => void;
};

export const useGatewayStore = create<GatewayState>((set, get) => {
  let detachStatus: (() => void) | null = null;

  const attachStatusBridge = (gateway: OrchestrationGateway): void => {
    if (detachStatus !== null) {
      detachStatus();
      detachStatus = null;
    }
    detachStatus = gateway.onStatus((next) => {
      set({ status: next });
    });
  };

  return {
    gateway: null,
    transport: null,
    status: 'idle',
    setGateway: (gateway, transport) => {
      Logger.info('coding-agent.store.gateway.set_gateway', { transport });
      const previous = get();
      if (previous.gateway !== null && previous.gateway !== gateway) {
        // The old gateway is detached from the store but not closed: its socket may stay open.
        Logger.warn('coding-agent.gateway.replaced', {
          transport,
          previousTransport: previous.transport,
        });
      }
      attachStatusBridge(gateway);
      set({
        gateway,
        transport,
        status: gateway.status(),
      });
    },
    connect: async () => {
      const { gateway } = get();
      if (gateway === null) {
        Logger.info('coding-agent.store.gateway.connect_skipped', {});
        return;
      }
      Logger.info('coding-agent.store.gateway.connect', {});
      try {
        await gateway.connect();
      } catch (cause) {
        Logger.withError(cause).warn(
          'coding-agent.store.gateway.connect_failed',
        );
      }
    },
    disconnect: () => {
      const { gateway } = get();
      Logger.info('coding-agent.store.gateway.disconnect', {});
      if (detachStatus !== null) {
        detachStatus();
        detachStatus = null;
      }
      if (gateway !== null) {
        try {
          gateway.close();
        } catch (cause) {
          Logger.withError(cause).warn(
            'coding-agent.store.gateway.disconnect_failed',
          );
        }
      }
      set({ gateway: null, transport: null, status: 'closed' });
    },
  };
});
