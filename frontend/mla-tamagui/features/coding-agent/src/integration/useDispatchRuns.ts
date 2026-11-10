// features/coding-agent/src/integration/useDispatchRuns.ts
//
// Integration hooks that bind the dispatch-runs section to a transport:
// `useDispatchRuns(transport)` polls the list every 5000 ms while mounted,
// `useDispatchRun(transport, runId)` polls the detail every 1500 ms while
// `details[runId]?.status === 'running'` (and once otherwise). The gateway
// itself is resolved with `useMemo` against the transport — `'fixture'`
// always binds to `createFixtureDispatchRunsGateway()`, anything else tries
// `resolveVmOpsApiBaseUrl()` and falls back to undefined when the runtime
// hasn't been configured, in which case `enabled` flips to false and the UI
// hides the section. Every selector reads one field from the store so the
// zustand v5 reference-equality check stays stable.
import { useEffect, useMemo } from 'react';
import { Logger } from '@lib/logging';
import type {
  DispatchRunDetail,
  DispatchRunMeta,
} from '../domain';

import {
  createFixtureDispatchRunsGateway,
  createHttpDispatchRunsGateway,
  dispatchRunDashboardUrl,
  resolveVmOpsApiBaseUrl,
  type DispatchRunsGateway,
} from '../data/dispatchRunsGateway';
import {
  useDispatchRunsStore,
  type DispatchRunsStatus,
} from '../state/dispatchRunsStore';
import type { CodingAgentTransport } from '../state/gatewayStore';

const LIST_POLL_MS = 5000;
const DETAIL_POLL_MS = 1500;

export type UseDispatchRunsResult = {
  runs: DispatchRunMeta[];
  status: DispatchRunsStatus;
  error?: string;
  enabled: boolean;
};

export type UseDispatchRunResult = {
  detail: DispatchRunDetail | undefined;
  status: DispatchRunsStatus;
  error?: string;
  dashboardUrl: string | undefined;
};

type ResolvedGateway = {
  gateway: DispatchRunsGateway | undefined;
  baseUrl: string | undefined;
};

function resolveGateway(transport: CodingAgentTransport): ResolvedGateway {
  if (transport === 'fixture') {
    return { gateway: createFixtureDispatchRunsGateway(), baseUrl: undefined };
  }
  const baseUrl = resolveVmOpsApiBaseUrl();
  if (baseUrl === undefined) {
    return { gateway: undefined, baseUrl: undefined };
  }
  return { gateway: createHttpDispatchRunsGateway(baseUrl), baseUrl };
}

// Without the base URL the section vanishes and a run screen stays empty: say why, once per mount.
function logDisabled(transport: CodingAgentTransport): void {
  Logger.info('coding-agent.dispatch_runs.disabled', {
    reason: 'vm-ops-api-base-url-unset',
    transport,
  });
}

export function useDispatchRuns(
  transport: CodingAgentTransport,
): UseDispatchRunsResult {
  const { gateway } = useMemo(
    () => resolveGateway(transport),
    [transport],
  );
  const runs = useDispatchRunsStore((state) => state.runs);
  const status = useDispatchRunsStore((state) => state.status);
  const error = useDispatchRunsStore((state) => state.error);

  useEffect(() => {
    if (gateway === undefined) {
      logDisabled(transport);
      return;
    }
    void useDispatchRunsStore.getState().loadRuns(gateway);
    const interval = setInterval(() => {
      void useDispatchRunsStore.getState().loadRuns(gateway);
    }, LIST_POLL_MS);
    return () => {
      clearInterval(interval);
    };
  }, [gateway, transport]);

  return {
    runs,
    status,
    error,
    enabled: gateway !== undefined,
  };
}

export function useDispatchRun(
  transport: CodingAgentTransport,
  runId: string,
): UseDispatchRunResult {
  const { gateway, baseUrl } = useMemo(
    () => resolveGateway(transport),
    [transport],
  );
  const detail = useDispatchRunsStore((state) => state.details[runId]);
  const status = useDispatchRunsStore((state) => state.status);
  const error = useDispatchRunsStore((state) => state.error);

  useEffect(() => {
    if (gateway === undefined) {
      logDisabled(transport);
      return;
    }
    void useDispatchRunsStore.getState().loadRun(gateway, runId);
  }, [gateway, runId, transport]);

  useEffect(() => {
    if (gateway === undefined) return;
    if (detail?.status !== 'running') return;
    const interval = setInterval(() => {
      void useDispatchRunsStore.getState().loadRun(gateway, runId);
    }, DETAIL_POLL_MS);
    return () => {
      clearInterval(interval);
    };
  }, [gateway, runId, detail?.status]);

  return {
    detail,
    status,
    error,
    dashboardUrl:
      baseUrl === undefined ? undefined : dispatchRunDashboardUrl(baseUrl, runId),
  };
}