import { randomUUID } from 'expo-crypto';
import { z } from 'zod';
import type { Pairing } from '../../domain/mobile';
import type { T3MobilePort } from '../../domain/t3Mobile';
import { readPairing, savePairing } from '../rca/credentials';
import { endpoint } from '../rca/endpoint';
import { boundedSchema, openConversation, threadSchema } from './session';

const key = 'rca.mobile.connection.t3.v2';
const shellSchema = z.object({
  projects: z.array(z.object({ id: z.string(), title: z.string() })),
  threads: z.array(threadSchema),
});

async function json(base: string, path: string, init?: RequestInit) {
  const response = await fetch(base + path, init);
  if (!response.ok)
    throw new Error(
      'The server declined the request (HTTP ' + response.status + ').'
    );
  return response.json() as Promise<unknown>;
}

const authorization = (pair: Pairing) => ({
  Authorization: 'Bearer ' + pair.token,
});

export function createT3MobileApi(): T3MobilePort {
  return {
    newId: randomUUID,
    restore: () => readPairing(key),
    async pair(host, code, signal) {
      const base = endpoint(host);
      z.object({ environmentId: z.string() }).parse(
        await json(base, '/.well-known/t3/environment', { signal })
      );
      const form = new URLSearchParams({
        grant_type: 'urn:ietf:params:oauth:grant-type:token-exchange',
        subject_token: code.trim(),
        subject_token_type:
          'urn:t3:params:oauth:token-type:environment-bootstrap',
        requested_token_type: 'urn:ietf:params:oauth:token-type:access_token',
      });
      const result = await json(base, '/oauth/token', {
        method: 'POST',
        signal,
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: form.toString(),
      });
      const { access_token: token } = z
        .object({ access_token: z.string().min(1) })
        .parse(result);
      const pairing = { endpoint: base, token };
      await json(base, '/api/auth/session', {
        signal,
        headers: authorization(pairing),
      });
      await savePairing(pairing, key);
      return pairing;
    },
    async sessionContext(pair, threadId, signal) {
      const path = '/api/orchestration/threads/' + encodeURIComponent(threadId) + '/session-context';
      const result = await json(endpoint(pair.endpoint), path, {
        signal,
        headers: { ...authorization(pair), 'x-t3-orchestration-protocol': '2' },
      });
      return z.object({
        workspace: z.string().nullable(),
        sessions: z.array(z.object({ id: z.string(), title: z.string() })),
      }).parse(result);
    },
    async shell(pair) {
      return shellSchema.parse(
        await json(endpoint(pair.endpoint), '/api/orchestration/shell', {
          headers: {
            ...authorization(pair),
            'x-t3-orchestration-protocol': '2',
          },
        })
      );
    },
    async open(pair, thread, update, disconnected) {
      const base = endpoint(pair.endpoint);
      const initial = boundedSchema.parse(
        await json(
          base,
          '/api/orchestration/threads/' +
            encodeURIComponent(thread) +
            '/bounded?rcaChatLayout=1',
          {
            headers: {
              ...authorization(pair),
              'x-t3-orchestration-protocol': '2',
            },
          }
        )
      );
      const result = await json(base, '/api/auth/websocket-ticket/rpc', {
        method: 'POST',
        headers: authorization(pair),
      });
      const { ticket } = z.object({ ticket: z.string().min(1) }).parse(result);
      const url = new URL(base.replace(/^http/, 'ws') + '/ws');
      url.searchParams.set('orchestrationProtocol', '2');
      url.searchParams.set('frameParts', '1');
      url.searchParams.set('wsTicket', ticket);
      return openConversation(
        new WebSocket(url.href),
        base,
        thread,
        update,
        disconnected,
        initial
      );
    },
  };
}
