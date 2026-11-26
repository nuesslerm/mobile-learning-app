import type { ChatImageFile, ImageActions } from './imageActions';
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

/** The theme a device asks for; "system" follows the device's own setting. */
export type AppearancePreference = 'system' | 'light' | 'dark';

/** The settings this app keeps on the device. */
export interface SettingsPreferencesPort {
  readAppearance(): Promise<AppearancePreference>;
  writeAppearance(preference: AppearancePreference): Promise<void>;
  /** Whether the user asked for agent notifications. */
  readNotificationsEnabled(): Promise<boolean>;
  writeNotificationsEnabled(enabled: boolean): Promise<void>;
  /** When the user last asked for an update check, or null when never. */
  readLastUpdateCheckAt(): Promise<number | null>;
  writeLastUpdateCheckAt(at: number): Promise<void>;
}

/** What an update check found. */
export type AppUpdateCheck =
  | { status: 'available'; updateId: string }
  | { status: 'up-to-date' }
  | { status: 'failed'; message: string };

/** The app's own update channel. */
export interface AppUpdatePort {
  /** Looks for a published update; it runs only when the user asks for it. */
  checkNow(): Promise<AppUpdateCheck>;
  /** Fetches and applies an update the check found. */
  install(updateId: string): Promise<void>;
}

/** The links that leave the app. */
export interface ExternalLinksPort {
  open(url: string): Promise<void>;
}

/** The app's own identity. */
export interface AppInfoPort {
  /** The app's version, e.g. "1.0.0"; null when the build carries none. */
  version(): Promise<string | null>;
  /** The native build number; null when the build carries none. */
  build(): Promise<string | null>;
}

export type PairingCameraPermission = { granted: boolean; canAskAgain: boolean };
export interface PairingCameraPort {
  get(): Promise<PairingCameraPermission>;
  request(): Promise<PairingCameraPermission>;
}

export type CodingAgentPorts = {
  imageActionsFor(image: ChatImageFile, signal: AbortSignal): ImageActions | Promise<ImageActions>;
  copyMessage(text: string): Promise<boolean>;
  pairingCamera: PairingCameraPort;
  t3Mobile?: T3MobilePort;
  mobile?: MobilePort;
  orchestration: OrchestrationPort;
  dispatchRuns: DispatchRunsPort;
  tokenStore: TokenStorePort;
  preferences: SettingsPreferencesPort;
  appUpdate: AppUpdatePort;
  appInfo: AppInfoPort;
  externalLinks: ExternalLinksPort;
};
