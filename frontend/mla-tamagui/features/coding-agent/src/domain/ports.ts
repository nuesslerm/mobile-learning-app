import type { T3MobilePort } from './t3Mobile';
import type { MobilePort } from './mobile';
import type {
  OrchestrationSubscribeShellItem,
  OrchestrationSubscribeThreadItem,
  ServerProbeResult,
} from '@app/remote-chat-agent-contracts/orchestration';

import type { DispatchRunDetail as DispatchRunDetailType, DispatchRunMeta } from './dispatchRuns';
import type {
  ClientOrchestrationCommand,
  OrchestrationCommandReceipt,
  OrchestrationThread,
  OrchestrationThreadSummary,
} from './orchestration';

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

export class DispatchRunsGatewayError extends Error {
  readonly status: number;
  constructor(message: string, status: number) {
    super(message);
    this.name = 'DispatchRunsGatewayError';
    this.status = status;
  }
}

export interface DispatchRunsGateway {
  listRuns(filter?: { agent?: string; q?: string }): Promise<DispatchRunMeta[]>;
  getRun(runId: string): Promise<DispatchRunDetailType>;
}

/** Where the orchestration gateway comes from: the live adapter resolves its config and opens a WebSocket, a test double opens an in-memory one. */
export type CodingAgentTransport = 'ws' | 'fixture';

export type OrchestrationPort = {
  transport: CodingAgentTransport;
  open(): Promise<OrchestrationGateway>;
};

export type DispatchRunsPort = {
  /** Undefined while the reporting base URL is unset: the section hides and a run screen stays empty. */
  gateway: DispatchRunsGateway | undefined;
  dashboardUrl(runId: string): string | undefined;
};

export type TokenStorePort = {
  read(): Promise<string | null>;
  write(token: string): Promise<void>;
  clear(): Promise<void>;
};

export type PairingCameraPermission = { granted: boolean; canAskAgain: boolean };
export interface PairingCameraPort {
  get(): Promise<PairingCameraPermission>;
  request(): Promise<PairingCameraPermission>;
}

export type CodingAgentPorts = {
  pairingCamera: PairingCameraPort;

  mobile?: MobilePort;
  t3Mobile?: T3MobilePort;
  orchestration: OrchestrationPort;
  dispatchRuns: DispatchRunsPort;
  tokenStore: TokenStorePort;
};
