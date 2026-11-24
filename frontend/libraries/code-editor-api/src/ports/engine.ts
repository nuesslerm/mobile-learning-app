import type { EditorEvent, EventType, Reply, RequestName, Requests } from './protocol';

export interface EditorEngine { // the driving port: the fake, the CodeMirror page and a remote page are interchangeable
  request<K extends RequestName>(name: K, params: Requests[K]['params']): Promise<Reply<Requests[K]['value']>>; // rejects only on transport failure
  listen(types: readonly EventType[], onEvent: (event: EditorEvent) => void): () => void;
}
