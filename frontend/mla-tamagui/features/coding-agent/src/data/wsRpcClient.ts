// features/coding-agent/src/data/wsRpcClient.ts
//
// Raw WebSocket transport for the orchestration server's RPC envelope.
// Owns the connection lifecycle (open, ping, reconnect, close) and the
// per-frame bookkeeping the higher-level gateway needs: a pending-request
// map keyed by request id, a subscriptions map keyed by subscription id
// that survives reconnects (thread subscriptions resume from the last
// sequence the client saw, shell subscriptions resume from scratch).
//
// Built on the `ClientEnvelopeSchema` / `ServerEnvelopeSchema` zod tables
// in `@app/remote-chat-agent-contracts`. Every inbound frame is zod-
// decoded — frames that fail to decode are logged with
// `coding-agent.ws.decode_failed` and dropped (the connection is kept;
// a malformed frame from a buggy server shouldn't yank the socket). The
// transport itself is generic over the envelope — the gateway layer
// (`orchestrationGateway.ts`) is responsible for method-specific result
// schemas.
//
// Reconnect uses `setTimeout`-based exponential backoff (500 → 8000 ms,
// doubled on every failure, reset to 500 ms once a connection is healthy)
// rather than `AbortSignal.timeout`: Hermes doesn't implement that API on
// the device, and the alternative would be a hard-to-reproduce crash on
// real hardware (the same trap `voiceEditTransport.ts` already escaped).
import { Logger } from '@lib/logging';
import { errorNameOf, wsUrlHost } from '../domain/wsUrl';
import {
  ServerEnvelopeSchema,
  type ClientEnvelope,
  type RequestId,
  type ServerEnvelope,
  type WsMethod,
  type WsSubscription,
} from '@app/remote-chat-agent-contracts/orchestration';

export type WsRpcClientStatus =
  | 'idle'
  | 'connecting'
  | 'open'
  | 'reconnecting'
  | 'closed';

type WsRpcStatusListener = (status: WsRpcClientStatus) => void;

export type CreateWsRpcClientOptions = {
  url: string;
  WebSocketImpl?: typeof WebSocket;
};

export interface WsRpcClient {
  send<M extends WsMethod>(method: M, params: unknown): Promise<unknown>;
  subscribe<S extends WsSubscription>(
    method: S,
    params: unknown,
    onItem: (item: unknown) => void,
  ): () => void;
  onStatus(cb: WsRpcStatusListener): () => void;
  close(): void;
}

const PING_INTERVAL_MS = 15_000;
const RECONNECT_BASE_MS = 500;
const RECONNECT_MAX_MS = 8_000;

type PendingRequest = {
  method: WsMethod;
  resolve: (value: unknown) => void;
  reject: (reason: unknown) => void;
};

type SubscriptionEntry = {
  method: WsSubscription;
  params: unknown;
  onItem: (item: unknown) => void;
  /** Highest sequence seen on this subscription's `event` items, if any. */
  maxSequence: number | undefined;
};

type SubscriptionItem = {
  kind?: string;
  event?: { sequence?: number };
};

type CloseDetails = { code?: number; reason?: string; wasClean?: boolean };

let clientCounter = 0;

function paramField(params: unknown, key: string): unknown {
  if (params === null || typeof params !== 'object') return undefined;
  return (params as Record<string, unknown>)[key];
}

export function createWsRpcClient(
  options: CreateWsRpcClientOptions,
): WsRpcClient {
  const { url, WebSocketImpl } = options;
  // The URL carries the bearer token as a query parameter: log the host and this id, never the URL.
  const clientId = ++clientCounter;
  const host = wsUrlHost(url);
  const WebSocketCtor =
    WebSocketImpl ?? (globalThis as { WebSocket?: typeof WebSocket }).WebSocket;
  if (WebSocketCtor === undefined) {
    throw new Error(
      'createWsRpcClient: no WebSocket implementation available (pass WebSocketImpl)',
    );
  }

  let status: WsRpcClientStatus = 'idle';
  let socket: WebSocket | null = null;
  let pingTimer: ReturnType<typeof setInterval> | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let backoffMs = RECONNECT_BASE_MS;
  let requestCounter = 0;
  let subscriptionCounter = 0;
  let explicitlyClosed = false;
  /** Sockets constructed since the last one that opened. */
  let attempt = 0;
  let openedAt: number | null = null;

  const pending = new Map<string, PendingRequest>();
  const subscriptions = new Map<string, SubscriptionEntry>();
  const statusListeners = new Set<WsRpcStatusListener>();
  /** Outbound envelopes waiting for the socket to open (or to recover). */
  const outboundQueue: ClientEnvelope[] = [];

  const setStatus = (next: WsRpcClientStatus): void => {
    if (status === next) return;
    status = next;
    for (const listener of statusListeners) {
      try {
        listener(next);
      } catch (cause) {
        Logger.withError(cause).warn(
          'coding-agent.ws.status_listener_error',
        );
      }
    }
  };

  const nextRequestId = (): RequestId => `r${++requestCounter}` as RequestId;
  const nextSubscriptionId = (): RequestId =>
    `s${++subscriptionCounter}` as RequestId;

  const sendNow = (envelope: ClientEnvelope): boolean => {
    if (socket === null || socket.readyState !== WebSocketCtor.OPEN) {
      return false;
    }
    try {
      socket.send(JSON.stringify(envelope));
      return true;
    } catch (cause) {
      Logger.withError(cause).withMetadata({ kind: envelope.kind }).warn(
        'coding-agent.ws.send_failed',
      );
      return false;
    }
  };

  const ensureConnection = (): void => {
    if (status === 'idle') {
      openSocket();
    }
  };

  const queueOrSend = (envelope: ClientEnvelope): void => {
    if (status === 'open' && sendNow(envelope)) return;
    outboundQueue.push(envelope);
    ensureConnection();
  };

  const flushQueue = (): void => {
    if (socket === null || socket.readyState !== WebSocketCtor.OPEN) return;
    const queue = outboundQueue.splice(0, outboundQueue.length);
    for (const envelope of queue) {
      if (!sendNow(envelope)) {
        outboundQueue.push(envelope);
        break;
      }
    }
  };

  const startPing = (): void => {
    stopPing();
    pingTimer = setInterval(() => {
      queueOrSend({ kind: 'ping', ts: Date.now() });
    }, PING_INTERVAL_MS);
  };

  const stopPing = (): void => {
    if (pingTimer !== null) {
      clearInterval(pingTimer);
      pingTimer = null;
    }
  };

  const scheduleReconnect = (): void => {
    if (explicitlyClosed) return;
    if (reconnectTimer !== null) return;
    setStatus('reconnecting');
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      openSocket();
    }, backoffMs);
    backoffMs = Math.min(backoffMs * 2, RECONNECT_MAX_MS);
  };

  const openSocket = (): void => {
    if (explicitlyClosed) return;
    if (status === 'open' || status === 'connecting') return;
    setStatus('connecting');
    attempt += 1;
    const startedAt = Date.now();
    try {
      socket = new WebSocketCtor(url);
    } catch (cause) {
      // An invalid-URL error quotes the URL, token included.
      Logger.withMetadata({
        clientId,
        host,
        errorName: errorNameOf(cause),
      }).warn('coding-agent.ws.constructor_failed');
      socket = null;
      scheduleReconnect();
      return;
    }

    socket.addEventListener('open', () => {
      openedAt = Date.now();
      Logger.info('coding-agent.ws.open', {
        clientId,
        host,
        attempt,
        connectMs: openedAt - startedAt,
        subscriptions: subscriptions.size,
        queued: outboundQueue.length,
      });
      attempt = 0;
      backoffMs = RECONNECT_BASE_MS;
      setStatus('open');
      startPing();
      // Resubscribe before flushing one-shot requests: a subscription may
      // produce the events the server uses to satisfy an earlier `getThread`
      // call, and we don't want a window where the shell list is empty
      // because the request beat the subscribe over the wire.
      resubscribeAll();
      flushQueue();
    });

    socket.addEventListener('message', (event) => {
      handleMessage(event.data);
    });

    socket.addEventListener('close', (event) => {
      const { code, reason, wasClean } = event as CloseDetails;
      Logger.info('coding-agent.ws.close', {
        clientId,
        host,
        code,
        wasClean,
        reason: reason?.slice(0, 80),
        openMs: openedAt === null ? null : Date.now() - openedAt,
        pending: pending.size,
        attempt,
        nextDelayMs: explicitlyClosed ? null : backoffMs,
      });
      openedAt = null;
      stopPing();
      socket = null;
      rejectAllPending(new Error('coding-agent.ws socket closed'));
      if (explicitlyClosed) {
        setStatus('closed');
      } else {
        scheduleReconnect();
      }
    });

    socket.addEventListener('error', (event) => {
      Logger.withMetadata({
        clientId,
        host,
        event: eventOrMessage(event),
      }).warn('coding-agent.ws.error');
    });
  };

  const handleMessage = (raw: unknown): void => {
    if (typeof raw !== 'string') return;
    let parsed: unknown;
    try {
      parsed = JSON.parse(raw);
    } catch (cause) {
      // A frame can carry an agent reply or a user message, and V8's parse error quotes it: log size and error name only.
      Logger.withMetadata({
        clientId,
        length: raw.length,
        errorName: errorNameOf(cause),
      }).warn('coding-agent.ws.decode_failed');
      return;
    }
    const result = ServerEnvelopeSchema.safeParse(parsed);
    if (!result.success) {
      Logger.withMetadata({
        clientId,
        length: raw.length,
        issues: result.error.issues.slice(0, 5),
      }).warn('coding-agent.ws.decode_failed');
      return;
    }
    handleEnvelope(result.data);
  };

  const handleEnvelope = (envelope: ServerEnvelope): void => {
    switch (envelope.kind) {
      case 'hello': {
        Logger.info('coding-agent.ws.hello', {
          serverVersion: envelope.serverVersion,
          protocolVersion: envelope.protocolVersion,
          heartbeatIntervalMs: envelope.heartbeatIntervalMs,
        });
        return;
      }
      case 'response': {
        const request = pending.get(envelope.id);
        if (request === undefined) {
          // A refused subscription answers on its own id, which `pending` never holds: without this line the screen just stays empty.
          const refused = subscriptions.get(envelope.id);
          if (refused !== undefined && !envelope.ok) {
            Logger.withMetadata({
              clientId,
              method: refused.method,
              threadId: paramField(refused.params, 'threadId'),
              code: envelope.error?.code,
              message: envelope.error?.message?.slice(0, 120),
            }).warn('coding-agent.stream.subscribe_failed');
          }
          return;
        }
        pending.delete(envelope.id);
        if (envelope.ok) {
          request.resolve(envelope.result);
          return;
        }
        // The wire refine pins `error` to defined when `ok: false`, but the
        // zod schema still types it as optional, so guard here rather than
        // carry a useless `?? new WsError(...)` fallback upstream.
        if (envelope.error === undefined) {
          request.reject(new Error('coding-agent.ws response missing error'));
          return;
        }
        const err = envelope.error;
        request.reject(err);
        Logger.withMetadata({
          method: request.method,
          code: err.code,
        }).warn('coding-agent.ws.request_failed');
        return;
      }
      case 'event': {
        const sub = subscriptions.get(envelope.id);
        if (sub === undefined) return;
        try {
          sub.onItem(envelope.item);
        } catch (cause) {
          Logger.withError(cause)
            .withMetadata({ method: sub.method })
            .warn('coding-agent.ws.item_handler_error');
        }
        trackSequence(sub, envelope.item);
        return;
      }
      case 'complete': {
        // Server-initiated end-of-stream for a subscription. The user-
        // facing handler is not called — `complete` carries no payload —
        // but the entry is dropped so a future resubscribe on reconnect
        // (if any) doesn't replay it against a server that's already
        // closed the stream.
        const ended = subscriptions.get(envelope.id);
        subscriptions.delete(envelope.id);
        if (ended !== undefined) {
          Logger.info('coding-agent.stream.complete', {
            clientId,
            method: ended.method,
            threadId: paramField(ended.params, 'threadId'),
            lastSequence: ended.maxSequence,
          });
        }
        return;
      }
      case 'pong':
        return;
    }
  };

  const trackSequence = (
    sub: SubscriptionEntry,
    item: unknown,
  ): void => {
    if (
      sub.method !== 'orchestration.subscribeThread' ||
      item === null ||
      typeof item !== 'object'
    ) {
      return;
    }
    const candidate = item as SubscriptionItem;
    if (candidate.kind !== 'event') return;
    const sequence = candidate.event?.sequence;
    if (typeof sequence !== 'number' || !Number.isFinite(sequence)) return;
    sub.maxSequence =
      sub.maxSequence === undefined
        ? sequence
        : Math.max(sub.maxSequence, sequence);
  };

  const rejectAllPending = (reason: unknown): void => {
    if (pending.size === 0) return;
    for (const request of pending.values()) {
      request.reject(reason);
    }
    pending.clear();
  };

  const logSubscribe = (
    method: WsSubscription,
    params: unknown,
    resumed: boolean,
  ): void => {
    Logger.info('coding-agent.stream.subscribe', {
      clientId,
      method,
      threadId: paramField(params, 'threadId'),
      afterSequence: paramField(params, 'afterSequence'),
      resumed,
    });
  };

  const resubscribeAll = (): void => {
    // A subscribe queued while the socket was closed is sent below, with its resume point: queued, it would go out twice.
    for (let i = outboundQueue.length - 1; i >= 0; i--) {
      const queued = outboundQueue[i];
      if (queued?.kind === 'subscribe' && subscriptions.has(queued.id)) {
        outboundQueue.splice(i, 1);
      }
    }
    for (const [id, entry] of subscriptions) {
      const params = nextSubscribeParams(entry);
      logSubscribe(entry.method, params, true);
      sendNow({ kind: 'subscribe', id: id as RequestId, method: entry.method, params });
    }
  };

  const nextSubscribeParams = (entry: SubscriptionEntry): unknown => {
    if (
      entry.method !== 'orchestration.subscribeThread' ||
      entry.maxSequence === undefined ||
      entry.params === null ||
      typeof entry.params !== 'object'
    ) {
      return entry.params;
    }
    return { ...(entry.params as Record<string, unknown>), afterSequence: entry.maxSequence + 1 };
  };

  return {
    send<M extends WsMethod>(method: M, params: unknown): Promise<unknown> {
      return new Promise<unknown>((resolve, reject) => {
        if (explicitlyClosed) {
          reject(new Error('coding-agent.ws client is closed'));
          return;
        }
        const id = nextRequestId();
        pending.set(id, {
          method,
          resolve: (value) => resolve(value),
          reject,
        });
        queueOrSend({ kind: 'request', id, method, params });
      });
    },
    subscribe<S extends WsSubscription>(
      method: S,
      params: unknown,
      onItem: (item: unknown) => void,
    ): () => void {
      const id = nextSubscriptionId();
      const entry: SubscriptionEntry = {
        method,
        params,
        onItem,
        maxSequence: undefined,
      };
      subscriptions.set(id, entry);
      logSubscribe(method, params, false);
      queueOrSend({ kind: 'subscribe', id, method, params });
      return () => {
        if (subscriptions.delete(id)) {
          queueOrSend({ kind: 'unsubscribe', id });
        }
      };
    },
    onStatus(cb: WsRpcStatusListener): () => void {
      statusListeners.add(cb);
      try {
        cb(status);
      } catch (cause) {
        Logger.withError(cause).warn(
          'coding-agent.ws.status_listener_error',
        );
      }
      return () => {
        statusListeners.delete(cb);
      };
    },
    close(): void {
      if (explicitlyClosed) return;
      explicitlyClosed = true;
      stopPing();
      if (reconnectTimer !== null) {
        clearTimeout(reconnectTimer);
        reconnectTimer = null;
      }
      rejectAllPending(new Error('coding-agent.ws client closed'));
      statusListeners.clear();
      subscriptions.clear();
      outboundQueue.length = 0;
      const closing = socket;
      socket = null;
      if (closing !== null) {
        try {
          closing.close();
        } catch (cause) {
          Logger.withError(cause).warn('coding-agent.ws.close_failed');
        }
      }
      setStatus('closed');
    },
  };
}

function eventOrMessage(event: Event): string {
  if ('message' in event) {
    const message = (event as { message?: unknown }).message;
    // A native socket error can quote the request URL, which carries the token.
    if (typeof message === 'string') {
      return message.replace(/token=[^&\s"']*/gi, 'token=[REDACTED]').slice(0, 200);
    }
  }
  return 'websocket error';
}
