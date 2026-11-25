export class ThreadNotFound extends Error {}

export type Pairing = { endpoint: string; token: string };
export type Workspace = { workspace: string; kind: string; path?: string; attached: boolean };
export type MobileThread = { id: string; title: string; workspace?: string; status: string };
export type RepositoryTask = { number: number; title: string };
export type ThreadContext = {
  thread: MobileThread;
  task: { workspace: string; number: number } | null;
  family: { threads: { thread: MobileThread; lineage?: { parent?: string } }[] };
};
export type AgentEvent = {
  seq: number; thread: string; type: string; turn?: string; role?: string; text?: string;
  tool?: string; args?: unknown; request?: string; approval?: string; prompt?: string;
  plan?: string; message?: string; path?: string; data?: string; call?: string;
};
export interface MobilePort {
  readPairing(): Promise<Pairing | null>;
  pair(endpoint: string, code: string): Promise<Pairing>;
  revoke(pair: Pairing): Promise<void>;
  workspaces(pair: Pairing): Promise<Workspace[]>;
  createWorkspace(pair: Pairing, repo: string, name: string, base: string): Promise<string>;
  threads(pair: Pairing, workspace: string): Promise<MobileThread[]>;
  tasks(pair: Pairing, workspace: string): Promise<RepositoryTask[]>;
  createThread(pair: Pairing, workspace: string, title: string, driver: string, task?: number): Promise<string>;
  context(pair: Pairing, thread: string): Promise<ThreadContext>;
  history(pair: Pairing, thread: string, after: number): Promise<{ events: AgentEvent[]; more: boolean }>;
  follow(pair: Pairing, thread: string, after: number, event: (event: AgentEvent) => void, closed: () => void): () => void;
  send(pair: Pairing, thread: string, id: string, prompt: string): Promise<void>;
  respond(pair: Pairing, thread: string, request: string, kind: 'approvals' | 'questions', value: string): Promise<void>;
  newId(): string;
}
