import type { RpcTransport } from '../ports/transport';
import { RpcError, decode, encode, PROTOCOL_VERSION, type RpcRequest, type RpcResponse } from './protocol';

export type RpcHandlers = Record<string, (params: unknown) => unknown | Promise<unknown>>;

export type RpcServerOptions = {
  /** A reply or an event that could not be sent (the other side is gone). */
  onError?: (error: unknown) => void;
};

export type RpcServer = {
  /** Sends an event to whoever subscribed to `topic` on the other side. */
  emit(topic: string, data?: unknown): void;
  close(): void;
};

export function createRpcServer(transport: RpcTransport, handlers: RpcHandlers, options: RpcServerOptions = {}): RpcServer {
  const answer = async (request: RpcRequest): Promise<RpcResponse> => {
    const run = Object.hasOwn(handlers, request.method) ? handlers[request.method] : undefined;
    if (!run) {
      return { v: PROTOCOL_VERSION, kind: 'res', id: request.id, ok: false, error: { code: 'unknown-method', message: `no handler for ${request.method}` } };
    }
    return Promise.resolve()
      .then(() => run(request.params))
      .then(
        (result): RpcResponse => ({ v: PROTOCOL_VERSION, kind: 'res', id: request.id, ok: true, result }),
        (error: unknown): RpcResponse => {
          const failure = error instanceof RpcError ? error : new RpcError('remote', error instanceof Error ? error.message : String(error));
          return { v: PROTOCOL_VERSION, kind: 'res', id: request.id, ok: false, error: { code: failure.code, message: failure.message, data: failure.data } };
        }
      );
  };

  const stop = transport.listen((raw) => {
    const decoded = decode(raw);
    if (decoded.type !== 'message' || decoded.message.kind !== 'req') return;
    void answer(decoded.message)
      .then((response) => transport.send(encode(response)))
      .catch((error: unknown) => options.onError?.(error));
  });

  return {
    emit(topic, data) {
      transport.send(encode({ v: PROTOCOL_VERSION, kind: 'evt', topic, data }));
    },
    close: stop,
  };
}
