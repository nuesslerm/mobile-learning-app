import type { CheckoutChanges, CheckoutCommit, CheckoutHistory, HistoryCursor } from './checkoutHistory';
import type { HomeSummary, CheckoutSessions, WorkspaceCatalog, CheckoutRow, CheckoutEdit } from './workspaceCatalog';
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
export type T3Effort = { id: string; choices: { slug: string; name: string }[]; current?: string };
export type T3Model = { slug: string; name: string; effort?: T3Effort };
export type T3Option = { id: string; value: string | boolean };
export type T3ModelOptions = T3Option[] | Record<string, unknown>;
export type T3Thread = {
  id: string;
  projectId: string;
  worktreePath?: string | null;
  title: string;
  providerInstanceId?: string | null;
  modelSelection?: { instanceId: string; model: string; options?: T3ModelOptions } | null;
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

export type T3ComposerEntry = { kind: 'command' | 'skill' | 'file'; token: string; description?: string; hint?: string; aliases?: readonly string[] };

export const resumeCommand = {
  kind: 'command', token: '/resume', aliases: ['sessions'], description: 'Available in the desktop app',
} as const;
export interface T3Conversation {
  listDirectory(path: string): Promise<{entries: {path: string; kind: 'file' | 'directory'}[]; truncated: boolean}>;
  readFile(path: string): Promise<{contents: string; truncated: boolean}>;
  readonly commands?: readonly T3ComposerEntry[];
  files(query: string): Promise<T3ComposerEntry[]>;
  catalog(): Promise<T3ComposerEntry[]>;
  models(instanceId: string): Promise<T3Model[]>;
  setModel(commandId: string, instanceId: string, model: string, option?: T3Option): Promise<void>;
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
  editWorkspace(pairing: Pairing, checkout: string, edit: CheckoutEdit): Promise<CheckoutRow>;
  home(pairing: Pairing, signal: AbortSignal): Promise<HomeSummary>;
  workspaces(pairing: Pairing, signal: AbortSignal): Promise<WorkspaceCatalog>;
  checkoutChanges(pairing: Pairing, id: string, signal: AbortSignal): Promise<CheckoutChanges>;
  checkoutCommit(pairing: Pairing, id: string, sha: string, signal: AbortSignal): Promise<CheckoutCommit>;
  checkoutHistory(pairing: Pairing, id: string, cursor: HistoryCursor | null, signal: AbortSignal): Promise<CheckoutHistory>;
  checkoutSessions(pairing: Pairing, id: string, signal: AbortSignal): Promise<CheckoutSessions>;
  shell(pairing: Pairing): Promise<T3Shell>;
  open(
    pairing: Pairing,
    threadId: string,
    update: (snapshot: T3Snapshot) => void,
    disconnected: (error: Error) => void
  ): Promise<T3Conversation>;
  newId(): string;
}
