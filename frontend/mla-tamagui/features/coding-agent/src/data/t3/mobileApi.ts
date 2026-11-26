import { checkoutChanges, checkoutCommit, checkoutHistory, CheckoutHeadChanged } from '../../domain/checkoutHistory';
import { checkoutRow, checkoutSessions, homeSummary, workspaceCatalog } from '../../domain/workspaceCatalog';
import { workspacePreferences } from '../../domain/workspacePreferences';
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

class HttpError extends Error {
  constructor(readonly status: number) {
    super('The server declined the request (HTTP ' + status + ').');
  }
}

async function json(base: string, path: string, init?: RequestInit) {
  const response = await fetch(base + path, init);
  if (!response.ok) throw new HttpError(response.status);
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
      const path = '/api/orchestration/threads/' + encodeURIComponent(threadId) + '/session-context?savedTabs=1';
      const result = await json(endpoint(pair.endpoint), path, {
        signal,
        headers: { ...authorization(pair), 'x-t3-orchestration-protocol': '2' },
      });
      return z.object({
        workspace: z.string().nullable(),
        sessions: z.array(z.object({ id: z.string(), title: z.string(), needsProjection: z.boolean().optional() })),
      }).parse(result);
    },
    async workspacePreferences(pair, signal) {
      const data = await json(endpoint(pair.endpoint), '/api/mobile/workspace-preferences', {
        headers: authorization(pair), signal,
      });
      return workspacePreferences.parse(data).preferences;
    },
    async saveWorkspacePreferences(pair, patch) {
      const data = await json(endpoint(pair.endpoint), '/api/mobile/workspace-preferences', {
        method: 'POST',
        headers: { ...authorization(pair), 'Content-Type': 'application/json' },
        body: JSON.stringify({ version: 1, preferences: patch }),
      });
      return workspacePreferences.parse(data).preferences;
    },
    async editWorkspace(pair, checkout, edit) {
      return checkoutRow.parse(await json(endpoint(pair.endpoint), '/api/mobile/workspaces', {
        method: 'POST', headers: { ...authorization(pair), 'Content-Type': 'application/json' },
        body: JSON.stringify({ checkout, ...edit }),
      }));
    },
    async home(pair, signal) {
      try {
        return homeSummary.parse(await json(endpoint(pair.endpoint), '/api/mobile/home', {
          headers: authorization(pair), signal,
        }));
      } catch (error) {
        if (!(error instanceof HttpError && error.status === 404)) throw error;
        const catalog = await this.workspaces(pair, signal);
        return { host: catalog.host, worktreeCount: catalog.checkouts.length };
      }
    },
    async workspaces(pair, signal) {
      return workspaceCatalog.parse(await json(endpoint(pair.endpoint), '/api/mobile/workspaces', { headers: authorization(pair), signal }));
    },
    async checkoutChanges(pair, id, signal) {
      const path = '/api/mobile/source-control/changes?' + new URLSearchParams({ checkout: id });
      return checkoutChanges.parse(await json(endpoint(pair.endpoint), path, { headers: authorization(pair), signal }));
    },
    async checkoutCommit(pair, id, sha, signal) {
      const path = '/api/mobile/source-control/commit?' + new URLSearchParams({ checkout: id, sha });
      return checkoutCommit.parse(await json(endpoint(pair.endpoint), path, { headers: authorization(pair), signal }));
    },
    async checkoutHistory(pair, id, cursor, signal) {
      const params = new URLSearchParams({ checkout: id });
      if (cursor) {
        params.set('head', cursor.head);
        params.set('offset', String(cursor.offset));
      }
      const path = '/api/mobile/source-control/history?' + params.toString();
      try {
        return checkoutHistory.parse(await json(endpoint(pair.endpoint), path, { headers: authorization(pair), signal }));
      } catch (error) {
        if (cursor && error instanceof HttpError && error.status === 409) {
          throw new CheckoutHeadChanged();
        }
        throw error;
      }
    },
    async checkoutSessions(pair, id, signal) {
      const path = '/api/mobile/workspaces?checkout=' + encodeURIComponent(id);
      return checkoutSessions.parse(await json(endpoint(pair.endpoint), path, { headers: authorization(pair), signal }));
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
      const projectRoots = new Map<string, string>();
      async function projectRoot(id: string) {
        const cached = projectRoots.get(id);
        if (cached) return cached;
        const { projects } = z.object({ projects: z.array(z.object({
          id: z.string(), workspaceRoot: z.string(),
        })) }).parse(await json(base, '/api/projects', { headers: authorization(pair) }));
        const root = projects.find(project => project.id === id)?.workspaceRoot;
        if (!root) throw new Error('The project directory is unavailable.');
        projectRoots.set(id, root);
        return root;
      }
      return openConversation(
        new WebSocket(url.href),
        base,
        thread,
        update,
        disconnected,
        initial,
        projectRoot
      );
    },
  };
}
