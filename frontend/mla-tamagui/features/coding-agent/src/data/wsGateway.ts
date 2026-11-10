// features/coding-agent/src/data/wsGateway.ts
//
// OrchestrationGateway implementation that talks to the real WebSocket
// server. Owns the mapping from `OrchestrationGateway`'s typed method set
// to the WS-RPC envelopes in `@app/remote-chat-agent-contracts/wsRpc.ts`:
// every RPC `send` round-trips through `createWsRpcClient`, and every
// `subscribe` forwards raw stream items (decoded upstream by the WS layer)
// to the gateway callback without further mutation. The `OrchestrationGateway`
// contract is the only public surface; the underlying client is internal.
//
// The gateway is created with an already-resolved `CodingAgentConfig` (see
// `codingAgentConfig.ts`) plus an optional injectable `WsRpcClient` for
// tests. The WS URL is augmented with the bearer token here — `resolveCodingAgentConfig`
// keeps the URL and the token as separate fields so cold-start can resolve
// them independently, but the wire expects the token inside `?token=`.
// `connect()` is a no-op when an injected client owns the lifecycle; for the
// default client it issues a cheap `server.probe` so the underlying socket
// transitions out of `idle` and the caller can observe `status === 'open'`
// before issuing turn-start commands.
import { Logger } from '@lib/logging';
import {
  OrchestrationDispatchCommandResultSchema,
  OrchestrationGetThreadResultSchema,
  OrchestrationListThreadsResultSchema,
  ServerProbeResultSchema,
  type ClientOrchestrationCommand,
  type OrchestrationCommandReceipt,
  type OrchestrationSubscribeShellItem,
  type OrchestrationSubscribeThreadItem,
  type OrchestrationThread,
  type OrchestrationThreadSummary,
  type ServerProbeResult,
} from '@app/remote-chat-agent-contracts/orchestration';

import { errorNameOf, wsUrlHost } from '../domain/wsUrl';
import type { CodingAgentConfig } from './codingAgentConfig';
import type {
  OrchestrationGateway,
  OrchestrationGatewayStatus,
  OrchestrationShellListener,
  OrchestrationStatusListener,
  OrchestrationThreadListener,
} from './orchestrationGateway';
import {
  createWsRpcClient,
  type WsRpcClient,
  type WsRpcClientStatus,
} from './wsRpcClient';

export type CreateWsGatewayConfig = CodingAgentConfig & {
  WebSocketImpl?: typeof WebSocket;
};

export type CreateWsGatewayOptions = {
  client?: WsRpcClient;
};

function buildWsUrl(wsUrl: string, token: string | undefined): string {
  if (token === undefined || token.length === 0) return wsUrl;
  try {
    const url = new URL(wsUrl);
    url.searchParams.set('token', token);
    return url.toString();
  } catch (cause) {
    // The parse error quotes the whole URL, and the URL may already carry a token.
    Logger.withMetadata({
      host: wsUrlHost(wsUrl),
      errorName: errorNameOf(cause),
    }).warn('coding-agent.ws.url_token_append_failed');
    return wsUrl;
  }
}

export function createWsGateway(
  config: CreateWsGatewayConfig,
  options: CreateWsGatewayOptions = {},
): OrchestrationGateway {
  const ownsClient = options.client === undefined;
  const client: WsRpcClient =
    options.client ??
    createWsRpcClient({
      url: buildWsUrl(config.wsUrl, config.token),
      ...(config.WebSocketImpl !== undefined
        ? { WebSocketImpl: config.WebSocketImpl }
        : {}),
    });

  let currentStatus: OrchestrationGatewayStatus = 'idle';
  const statusListeners = new Set<OrchestrationStatusListener>();
  const relayClientStatus = (): (() => void) => {
    if (!ownsClient) return () => undefined;
    return client.onStatus((next: WsRpcClientStatus) => {
      currentStatus = next;
      for (const listener of statusListeners) {
        try {
          listener(next);
        } catch (cause) {
          Logger.withError(cause).warn(
            'coding-agent.ws.status_listener_error',
          );
        }
      }
    });
  };
  const unsubClientStatus = relayClientStatus();

  const close = (): void => {
    for (const listener of statusListeners) {
      statusListeners.delete(listener);
    }
    unsubClientStatus();
    if (ownsClient) {
      client.close();
    }
  };

  return {
    async connect(): Promise<void> {
      if (!ownsClient) return;
      if (currentStatus === 'open') return;
      try {
        await client.send('server.probe', {});
      } catch (cause) {
        Logger.withError(cause).withMetadata({
          host: wsUrlHost(config.wsUrl),
        }).warn('coding-agent.ws.connect_probe_failed');
      }
    },
    close,
    status() {
      return currentStatus;
    },
    onStatus(cb: OrchestrationStatusListener): () => void {
      statusListeners.add(cb);
      try {
        cb(currentStatus);
      } catch (cause) {
        Logger.withError(cause).warn(
          'coding-agent.ws.status_listener_error',
        );
      }
      return () => {
        statusListeners.delete(cb);
      };
    },
    async probe(): Promise<ServerProbeResult> {
      const result = await client.send('server.probe', {});
      return ServerProbeResultSchema.parse(result);
    },
    async listThreads(): Promise<OrchestrationThreadSummary[]> {
      const result = await client.send('orchestration.listThreads', {});
      return OrchestrationListThreadsResultSchema.parse(result).threads;
    },
    async getThread(id: string): Promise<OrchestrationThread> {
      const result = await client.send('orchestration.getThread', {
        threadId: id,
      });
      return OrchestrationGetThreadResultSchema.parse(result);
    },
    subscribeShell(cb: OrchestrationShellListener): () => void {
      return client.subscribe(
        'orchestration.subscribeShell',
        {},
        (item: unknown) => {
          cb(item as OrchestrationSubscribeShellItem);
        },
      );
    },
    subscribeThread(
      threadId: string,
      cb: OrchestrationThreadListener,
      afterSequence?: number,
    ): () => void {
      const params: { threadId: string; afterSequence?: number } = {
        threadId,
      };
      if (afterSequence !== undefined) {
        params.afterSequence = afterSequence;
      }
      return client.subscribe(
        'orchestration.subscribeThread',
        params,
        (item: unknown) => {
          cb(item as OrchestrationSubscribeThreadItem);
        },
      );
    },
    async dispatch(
      cmd: ClientOrchestrationCommand,
    ): Promise<OrchestrationCommandReceipt> {
      const result = await client.send('orchestration.dispatchCommand', cmd);
      return OrchestrationDispatchCommandResultSchema.parse(result);
    },
  };
}
