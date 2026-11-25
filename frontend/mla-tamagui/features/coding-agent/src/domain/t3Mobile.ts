import type { Pairing } from './mobile';

export type T3Message = {
  id: string;
  role: 'user' | 'assistant' | 'system';
  text: string;
};
export type T3Thread = { id: string; projectId: string; title: string };
export type T3Snapshot = { thread: T3Thread; messages: T3Message[] };
export type T3Shell = {
  projects: { id: string; title: string }[];
  threads: T3Thread[];
};

export interface T3Conversation {
  send(commandId: string, text: string): Promise<void>;
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
