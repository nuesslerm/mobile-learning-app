// features/coding-agent/src/state/serverConfigStore.ts
//
// Zustand store for the cached `server.probe` result and the derived
// model-picker options. `load(gateway)` round-trips through the gateway's
// `probe()` call; the settings sheet then calls `modelOptions(current)`
// with the currently-bound `ModelSelection` so the picker can label the
// active model and surface CLIs the thread is bound to but the probe
// doesn't list. The wire `ServerProbeResult` is converted to the
// `ServerProbeSummary` shape that `buildModelOptions` was ported against
// — different shape, same data, kept inside this store so the contract
// surface stays narrow.
import { Logger } from '@lib/logging';
import {
  buildModelOptions,
  type CliModelGroup,
  type ServerProbeModelDescriptor,
  type ServerProbeSummary,
  type ModelSelection,
} from '../domain';
import { ProviderName } from '@app/remote-chat-agent-contracts/orchestration';
import { create } from 'zustand';
import type { ServerProbeResult } from '@app/remote-chat-agent-contracts/orchestration';

import type { OrchestrationGateway } from '../data/orchestrationGateway';

type ServerConfigState = {
  probe: ServerProbeResult | null;
  loading: boolean;
  load: (gateway: OrchestrationGateway) => Promise<void>;
  modelOptions: (current: ModelSelection | null) => CliModelGroup[];
};

function probeToSummary(probe: ServerProbeResult): ServerProbeSummary {
  const models: Partial<
    Record<ProviderName, readonly ServerProbeModelDescriptor[]>
  > = {};
  for (const provider of probe.providers) {
    if (provider.models.length === 0) continue;
    models[provider.cli] = provider.models.map((model) => ({
      modelID: model.modelID,
      name: model.name,
      providerID: model.providerID,
      ...(model.isDefault !== undefined ? { isDefault: model.isDefault } : {}),
    }));
  }
  return {
    availableClis: probe.availableClis,
    selectedCli: probe.selectedCli,
    ...(Object.keys(models).length > 0 ? { models } : {}),
  };
}

export const useServerConfigStore = create<ServerConfigState>((set, get) => ({
  probe: null,
  loading: false,
  load: async (gateway) => {
    Logger.info('coding-agent.store.server_config.load', {});
    set({ loading: true });
    try {
      const probe = await gateway.probe();
      Logger.info('coding-agent.store.server_config.loaded', {
        availableClis: probe.availableClis,
        selectedCli: probe.selectedCli,
      });
      set({ probe, loading: false });
    } catch (cause) {
      Logger.withError(cause).warn(
        'coding-agent.store.server_config.load_failed',
      );
      set({ loading: false });
    }
  },
  modelOptions: (current) => {
    const { probe } = get();
    if (probe === null) return [];
    return buildModelOptions(probeToSummary(probe), current);
  },
}));
