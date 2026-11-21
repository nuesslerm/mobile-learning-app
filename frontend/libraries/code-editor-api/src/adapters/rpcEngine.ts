import { RpcError, type RpcClient, type RpcErrorCode } from '@lib/webview-rpc';

import type { EditorEngine } from '../ports/engine';
import type { EditorEvent, EventType, Reply, RequestName, Requests } from '../ports/protocol';

export class EditorTransportError extends Error {
  readonly code: RpcErrorCode | 'restarted';
  constructor(code: RpcErrorCode | 'restarted', message: string) {
    super(message);
    this.name = 'EditorTransportError';
    this.code = code;
  }
}

type Listener = { readonly types: ReadonlySet<EventType>; readonly fn: (event: EditorEvent) => void };

/**
 * The host's view of an editor in a WebView: requests become rpc calls, local listeners share one wire subscription (the union of their
 * types plus `ready`), and a `ready` event with a new instance fails every call still waiting, because the revisions it was made against are gone.
 */
export function toRemoteEngine(rpc: RpcClient, logError: (error: unknown) => void): EditorEngine {
  const listeners = new Set<Listener>();
  const waiting = new Set<(error: EditorTransportError) => void>();
  let onWire = '';
  let instance: string | null = null;

  const dispatch = (event: EditorEvent) => {
    for (const listener of [...listeners]) {
      if (!listener.types.has(event.type)) continue;
      try {
        listener.fn(event);
      } catch (error) {
        logError(error);
      }
    }
  };
  rpc.subscribe('editor.event', (data) => {
    if (typeof data !== 'object' || data === null || typeof (data as { type?: unknown }).type !== 'string') return;
    const event = data as EditorEvent;
    if (event.type === 'ready') {
      if (instance !== null && instance !== event.instance) for (const reject of [...waiting]) reject(new EditorTransportError('restarted', 'the editor was created again while the call was waiting'));
      instance = event.instance;
    }
    dispatch(event);
  });

  const syncWire = () => {
    const wanted = new Set<EventType>(['ready']);
    for (const listener of listeners) for (const type of listener.types) wanted.add(type);
    const key = [...wanted].sort().join(',');
    if (key === onWire) return;
    onWire = key;
    rpc.call('editor.subscribe', { types: [...wanted] }).then(undefined, (error: unknown) => {
      onWire = '';
      logError(error);
    });
  };

  const request = (name: RequestName, params: unknown): Promise<Reply<unknown>> => {
    syncWire();
    return new Promise<Reply<unknown>>((resolve, reject) => {
      const fail = (error: EditorTransportError) => reject(error);
      waiting.add(fail);
      rpc.call<Reply<unknown>>(`editor.${name}`, params).then(
        (reply) => {
          waiting.delete(fail);
          if (name === 'hello' && reply.ok) instance = (reply.value as { instance: string }).instance;
          resolve(reply);
        },
        (error: unknown) => {
          waiting.delete(fail);
          if (error instanceof RpcError && error.code === 'unknown-method') {
            resolve({ ok: false, revision: null, error: { code: 'unsupported', message: `this editor has no request ${name}`, what: `request:${name}` } });
            return;
          }
          const code = error instanceof RpcError ? error.code : 'protocol';
          reject(new EditorTransportError(code, error instanceof Error ? error.message : String(error)));
        },
      );
    });
  };

  return {
    request: request as EditorEngine['request'] & ((name: RequestName, params: Requests[RequestName]['params']) => Promise<Reply<unknown>>),
    listen(types, onEvent) {
      const listener: Listener = { types: new Set(types), fn: onEvent };
      listeners.add(listener);
      syncWire();
      return () => {
        listeners.delete(listener);
        syncWire();
      };
    },
  };
}
