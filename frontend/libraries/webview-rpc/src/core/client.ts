import type { RpcTransport } from '../ports/transport';
import { RpcError, decode, encode, PROTOCOL_VERSION, type RpcResponse } from './protocol';
import { realTimers, type Timers } from './timers';

export type RpcClientOptions = {
  /** How long a call waits for its answer before it fails with a `timeout` error. */
  timeoutMs?: number;
  timers?: Timers;
  /** The clock that tells a timer that fired late (the thread was away) from one that fired on time; the wall clock by default. */
  now?: () => number;
  /** A message in a protocol version this side does not speak. */
  onProtocolError?: (error: RpcError) => void;
};

export type CallOptions = { timeoutMs?: number };

export type RpcClient = {
  call<T = unknown>(method: string, params?: unknown, options?: CallOptions): Promise<T>;
  /** Hears the events the other side emits on `topic`; the returned function stops it. */
  subscribe(topic: string, listener: (data: unknown) => void): () => void;
  /** Fails every call still waiting with `closed` and stops listening. */
  close(): void;
};

const DEFAULT_TIMEOUT_MS = 10_000;
// a timer that fires this long after its time says the thread was away, not that the page did not answer
const STALL_MS = 500;

type Pending = { resolve: (value: unknown) => void; reject: (error: RpcError) => void; timer: unknown; stalled: boolean };

export function createRpcClient(transport: RpcTransport, options: RpcClientOptions = {}): RpcClient {
  const timers = options.timers ?? realTimers;
  const now = options.now ?? (() => Date.now());
  const pending = new Map<number, Pending>();
  const topics = new Map<string, Set<(data: unknown) => void>>();
  let nextId = 1;
  let closed = false;

  const settle = (message: RpcResponse) => {
    const entry = pending.get(message.id);
    if (!entry) return;
    pending.delete(message.id);
    timers.clear(entry.timer);
    if (message.ok) entry.resolve(message.result);
    else entry.reject(new RpcError(message.error.code, message.error.message, message.error.data));
  };

  const stop = transport.listen((raw) => {
    const decoded = decode(raw);
    if (decoded.type === 'version') {
      options.onProtocolError?.(new RpcError('protocol', `the other side speaks protocol ${String(decoded.version)}, this one ${PROTOCOL_VERSION}`));
      return;
    }
    if (decoded.type !== 'message') return;
    const { message } = decoded;
    if (message.kind === 'res') settle(message);
    else if (message.kind === 'evt') for (const listener of [...(topics.get(message.topic) ?? [])]) listener(message.data);
  });

  return {
    call<T>(method: string, params?: unknown, callOptions: CallOptions = {}): Promise<T> {
      if (closed) return Promise.reject(new RpcError('closed', `the channel is closed: ${method} was not sent`));
      const id = nextId++;
      const timeoutMs = callOptions.timeoutMs ?? options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
      return new Promise<T>((resolve, reject) => {
        const sentAt = now();
        const expire = () => {
          const entry = pending.get(id);
          if (!entry) return;
          // the answer may be queued right behind a timer the thread was too busy to run on time: the call then waits its whole time once more
          if (!entry.stalled && now() - sentAt - timeoutMs > STALL_MS) {
            entry.stalled = true;
            entry.timer = timers.set(expire, timeoutMs);
            return;
          }
          pending.delete(id);
          reject(new RpcError('timeout', `${method} got no answer in ${timeoutMs} ms`));
        };
        const timer = timers.set(expire, timeoutMs);
        pending.set(id, { resolve: resolve as (value: unknown) => void, reject, timer, stalled: false });
        try {
          transport.send(encode({ v: PROTOCOL_VERSION, kind: 'req', id, method, params }));
        } catch (error) {
          pending.delete(id);
          timers.clear(timer);
          // thrown inside the executor, so the call rejects with it
          throw error instanceof RpcError ? error : new RpcError('closed', error instanceof Error ? error.message : String(error));
        }
      });
    },
    subscribe(topic, listener) {
      const set = topics.get(topic) ?? new Set();
      set.add(listener);
      topics.set(topic, set);
      return () => {
        set.delete(listener);
      };
    },
    close() {
      if (closed) return;
      closed = true;
      stop();
      for (const [id, entry] of pending) {
        pending.delete(id);
        timers.clear(entry.timer);
        entry.reject(new RpcError('closed', 'the channel closed before the answer came'));
      }
    },
  };
}
