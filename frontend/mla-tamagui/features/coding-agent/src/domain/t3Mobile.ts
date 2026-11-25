import type { Pairing } from './mobile';

export type T3Attachment = {
  id: string;
  type: 'image' | 'file';
  name: string;
  mimeType: string;
};
export type T3ImageSource = { uri: string; expiresAt: number };

export type T3Message = {
  id: string;
  role: 'user' | 'assistant' | 'system';
  runId?: string | null;
  text: string;
  attachments?: T3Attachment[];
};
export type T3Thread = { id: string; projectId: string; title: string };
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
export type T3Shell = {
  projects: { id: string; title: string }[];
  threads: T3Thread[];
};

export interface T3Conversation {
  send(commandId: string, text: string): Promise<void>;
  resolveImage(attachment: T3Attachment): Promise<T3ImageSource>;
  close(): void;
}

export interface T3MobilePort {
  restore(): Promise<Pairing | null>;
  pair(endpoint: string, code: string): Promise<Pairing>;
  shell(pairing: Pairing): Promise<T3Shell>;
  open(
    pairing: Pairing,
    threadId: string,
    update: (snapshot: T3Snapshot) => void,
    disconnected: (error: Error) => void
  ): Promise<T3Conversation>;
  newId(): string;
}
