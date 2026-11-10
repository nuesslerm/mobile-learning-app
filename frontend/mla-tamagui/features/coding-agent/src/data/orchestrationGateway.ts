// features/coding-agent/src/data/orchestrationGateway.ts
//
// Typed surface the @feature/coding-agent UI uses to talk to the
// orchestration WebSocket server. Pure types + zod schemas (via
// `src/contracts`) — no IO, no React. Lives in `data/` because the gateway owns
// exactly one boundary (the wire schema), and per the contract data/ and
// integration/ files are not unit-tested from `tests/unit`; the fakes
// live under `src/testing` and the state layer injects this interface.
import type {
  ClientOrchestrationCommand,
  OrchestrationCommandReceipt,
  OrchestrationThread,
  OrchestrationThreadSummary,
} from '../domain';
import type {
  OrchestrationSubscribeShellItem,
  OrchestrationSubscribeThreadItem,
  ServerProbeResult,
} from '@app/remote-chat-agent-contracts/orchestration';

export type OrchestrationGatewayStatus =
  | 'idle'
  | 'connecting'
  | 'open'
  | 'reconnecting'
  | 'closed';

export type OrchestrationStatusListener = (
  status: OrchestrationGatewayStatus,
) => void;

export type OrchestrationShellListener = (
  item: OrchestrationSubscribeShellItem,
) => void;

export type OrchestrationThreadListener = (
  item: OrchestrationSubscribeThreadItem,
) => void;

export interface OrchestrationGateway {
  connect(): Promise<void>;
  close(): void;
  status(): OrchestrationGatewayStatus;
  probe(): Promise<ServerProbeResult>;
  listThreads(): Promise<OrchestrationThreadSummary[]>;
  getThread(id: string): Promise<OrchestrationThread>;
  subscribeShell(cb: OrchestrationShellListener): () => void;
  subscribeThread(
    threadId: string,
    cb: OrchestrationThreadListener,
    afterSequence?: number,
  ): () => void;
  dispatch(cmd: ClientOrchestrationCommand): Promise<OrchestrationCommandReceipt>;
  onStatus(cb: OrchestrationStatusListener): () => void;
}
