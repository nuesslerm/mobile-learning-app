import { createRpcServer, type RpcHandlers, type RpcServer, type RpcTransport } from '@lib/webview-rpc';

import type { EditorEngine } from '../ports/engine';
import { REQUEST_NAMES, type EventType } from '../ports/protocol';

const EVENT_TYPES: readonly EventType[] = ['ready', 'changed', 'selection', 'focus', 'completion', 'tree'];

/** Serves an engine over a page transport: one rpc method per request, whose result is the whole reply, and the events the host subscribed to. */
export function serveEditor(engine: EditorEngine, transport: RpcTransport, logError: (error: unknown) => void): RpcServer {
  let server: RpcServer | null = null;
  let stopListening: (() => void) | null = null;
  const handlers: RpcHandlers = {
    'editor.subscribe': (params) => {
      const asked = typeof params === 'object' && params !== null && Array.isArray((params as { types?: unknown }).types) ? ((params as { types: unknown[] }).types) : [];
      const types = EVENT_TYPES.filter((type) => asked.includes(type));
      stopListening?.();
      stopListening = types.length > 0 ? engine.listen(types, (event) => server?.emit('editor.event', event)) : null;
      return { types };
    },
  };
  for (const name of REQUEST_NAMES) handlers[`editor.${name}`] = (params) => engine.request(name, params as never);
  server = createRpcServer(transport, handlers, { onError: logError });
  return {
    emit: (topic, data) => server?.emit(topic, data),
    close: () => {
      stopListening?.();
      server?.close();
    },
  };
}
