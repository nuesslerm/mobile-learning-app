import type { WorkspacePatch, WorkspacePreferences } from './workspacePreferences';
import type { Pairing } from './mobile';

export type T3Attachment = {
  id: string;
  type: 'image' | 'file';
  name: string;
  mimeType: string;
};
export type T3ImageSource = { uri: string; expiresAt: number };

export type T3Message = {
  createdAt?: string;
  id: string;
  role: 'user' | 'assistant' | 'system';
  runId?: string | null;
  text: string;
  attachments?: T3Attachment[];
};
export type T3RuntimeMode = 'approval-required' | 'auto-accept-edits' | 'auto' | 'full-access';
export type T3Model = { slug: string; name: string };
export type T3Thread = {
  id: string;
  projectId: string;
  title: string;
  providerInstanceId?: string | null;
  modelSelection?: { instanceId: string; model: string } | null;
  runtimeMode?: T3RuntimeMode | null;
};
export type T3TurnItem = {
  id: string;
  type: string;
  runId?: string | null;
  messageId?: string | null;
  name?: string | null;
  text?: string | null;
  input?: unknown;
  output?: unknown;
  status?: string;
};
export type T3Snapshot = {
  thread: T3Thread;
  messages: T3Message[];
  turnItems?: T3TurnItem[];
  toolTextOffsets?: Record<string, number>;
};
export type SessionContext = { workspace: string | null; sessions: { id: string; title: string; needsProjection?: boolean }[] };
export type T3Shell = {
  projects: { id: string; title: string }[];
  threads: T3Thread[];
};

export interface T3Conversation {
  models(instanceId: string): Promise<T3Model[]>;
  setModel(commandId: string, instanceId: string, model: string): Promise<void>;
  setRuntimeMode(commandId: string, runtimeMode: T3RuntimeMode): Promise<void>;
  openTab(tabId: string): Promise<void>;
  send(commandId: string, text: string): Promise<void>;
  resolveImage(attachment: T3Attachment): Promise<T3ImageSource>;
  close(): void;
}

export interface T3MobilePort {
  workspacePreferences(pairing: Pairing, signal: AbortSignal): Promise<WorkspacePreferences>;
  saveWorkspacePreferences(pairing: Pairing, patch: WorkspacePatch): Promise<WorkspacePreferences>;
  restore(): Promise<Pairing | null>;
  pair(endpoint: string, code: string, signal?: AbortSignal): Promise<Pairing>;
  sessionContext(pairing: Pairing, threadId: string, signal: AbortSignal): Promise<SessionContext>;
  shell(pairing: Pairing): Promise<T3Shell>;
  open(
    pairing: Pairing,
    threadId: string,
    update: (snapshot: T3Snapshot) => void,
    disconnected: (error: Error) => void
  ): Promise<T3Conversation>;
  newId(): string;
}
