// features/coding-agent/src/state/useShellSubscription.ts
//
// Integration hook that wires the shell list subscription to the active
// `OrchestrationGateway`: forwards every `OrchestrationSubscribeShellItem`
// into `useThreadShellStore.applyShellItem`, which handles the snapshot /
// event projection. The WS layer resubscribes on reconnect internally
// (thread subscriptions also resume with `afterSequence`; the shell stream
// always resumes from scratch). Returns the current
// `OrchestrationGatewayStatus` so the thread-list screen can render a
// connecting banner without touching the store.
import { useEffect } from 'react';

import type {
  OrchestrationGatewayStatus,
  OrchestrationShellListener,
} from '../data/orchestrationGateway';
import { useGatewayStore } from '../state/gatewayStore';
import { useThreadShellStore } from '../state/threadShellStore';

export function useShellSubscription(): OrchestrationGatewayStatus {
  const gateway = useGatewayStore((s) => s.gateway);
  const status = useGatewayStore((s) => s.status);

  useEffect(() => {
    if (gateway === null) return;
    const apply: OrchestrationShellListener = (item) => {
      useThreadShellStore.getState().applyShellItem(item);
    };
    const detach = gateway.subscribeShell(apply);
    return () => {
      detach();
    };
  }, [gateway]);

  return status;
}