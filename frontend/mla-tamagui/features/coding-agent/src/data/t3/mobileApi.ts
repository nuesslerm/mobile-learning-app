import { authorization, HttpError, json } from './http';
import { checkoutPullRequest, checkoutBranches, checkoutFile, checkoutChanges, checkoutCommit, checkoutHistory, CheckoutHeadChanged } from '../../domain/checkoutHistory';
import { checkoutRow, checkoutSessions, desktopAgents, homeSummary, workspaceCatalog, workspaceStatuses } from '../../domain/workspaceCatalog';
import { workspacePreferences } from '../../domain/workspacePreferences';
import { randomUUID } from 'expo-crypto';
import { z } from 'zod';
import { WorktreeMissing, type T3MobilePort } from '../../domain/t3Mobile';
import { readPairing, savePairing } from '../rca/credentials';
import { endpoint } from '../rca/endpoint';
import { boundedSchema, openConversation, threadSchema } from './session';

const key = 'rca.mobile.connection.t3.v2';
const shellSchema = z.object({
  projects: z.array(z.object({ id: z.string(), title: z.string() })),
  threads: z.array(threadSchema),
});

const disclosureRoute = '/api/mobile/workspace-disclosure';
const disclosureKeys = z.array(z.string());

export function createT3MobileApi(): T3MobilePort {
  const disclosureWrites = new Map<string, Promise<void>>();
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
    async agents(pair, signal) {
      return desktopAgents.parse(await json(endpoint(pair.endpoint), '/api/mobile/agents', { headers: authorization(pair), signal }));
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
    async workspaceDisclosure(pair, signal) {
      await disclosureWrites.get(pair.endpoint);
      return disclosureKeys.parse(await json(endpoint(pair.endpoint), disclosureRoute, { headers: authorization(pair), signal }));
    },
    setWorkspaceDisclosure(pair, key, collapsed) {
      const write = async () => disclosureKeys.parse(await json(endpoint(pair.endpoint), disclosureRoute, {
        method: 'POST', headers: { ...authorization(pair), 'Content-Type': 'application/json' },
        body: JSON.stringify({ key, collapsed }),
      }));
      const result = (disclosureWrites.get(pair.endpoint) ?? Promise.resolve()).then(write);
      const settled = result.then(cleanup, cleanup);
      function cleanup() {
        if (disclosureWrites.get(pair.endpoint) === settled) disclosureWrites.delete(pair.endpoint);
      }
      disclosureWrites.set(pair.endpoint, settled);
      return result;
    },
    async workspaceStatuses(pair, signal) {
      return workspaceStatuses.parse(await json(endpoint(pair.endpoint), '/api/mobile/workspace-statuses', { headers: authorization(pair), signal }));
    },
    async workspaces(pair, signal) {
      return workspaceCatalog.parse(await json(endpoint(pair.endpoint), '/api/mobile/workspaces', { headers: authorization(pair), signal }));
    },
    async threadCheckout(pair, thread, selected, signal) {
      const path = '/api/mobile/source-control/checkout?thread=' + encodeURIComponent(thread) +
        (selected ? '&checkout=' + encodeURIComponent(selected) : '');
      try {
        return checkoutRow.parse(await json(endpoint(pair.endpoint), path, { headers: authorization(pair), signal }));
      } catch (cause) {
        if (cause instanceof HttpError && cause.status === 404 && selected) {
          const catalog = await this.workspaces(pair, signal);
          if (!catalog.checkouts.some(row => row.id === selected && !row.missing)) throw new WorktreeMissing();
        }
        throw cause;
      }
    },
    async linkCheckoutPullRequest(pair, checkout, number) {
      return checkoutPullRequest.parse(await json(endpoint(pair.endpoint), '/api/mobile/source-control/pull-request', {
        method: 'POST', headers: { ...authorization(pair), 'Content-Type': 'application/json' }, body: JSON.stringify({checkout,number}),
      }));
    },
    async checkoutPullRequest(pair, id, signal) {
      const path = '/api/mobile/source-control/pull-request?checkout=' + encodeURIComponent(id);
      return checkoutPullRequest.parse(await json(endpoint(pair.endpoint), path, { headers: authorization(pair), signal }));
    },
    async checkoutBranches(pair, id, signal) {
      const path = '/api/mobile/source-control/branches?checkout=' + encodeURIComponent(id);
      return checkoutBranches.parse(await json(endpoint(pair.endpoint), path, { headers: authorization(pair), signal }));
    },
    async switchCheckoutBranch(pair, id, branch, signal) {
      const response = await fetch(endpoint(pair.endpoint) + '/api/mobile/source-control/branches', {
        method: 'POST',
        headers: { ...authorization(pair), 'Content-Type': 'application/json' },
        body: JSON.stringify({ checkout: id, branch }),
        signal,
      });
      if (!response.ok) {
        if (response.status === 409) {
          const conflict = z.object({ message: z.string() }).safeParse(await response.json());
          if (conflict.success) throw new Error(conflict.data.message);
        }
        throw new HttpError(response.status);
      }
      return z.object({ branch: z.string(), head: z.string() }).parse(await response.json());
    },
    async checkoutChanges(pair, id, signal) {
      const path = '/api/mobile/source-control/changes?' + new URLSearchParams({ checkout: id });
      try {
        return checkoutChanges.parse(await json(endpoint(pair.endpoint), path, { headers: authorization(pair), signal }));
      } catch (cause) {
        if (cause instanceof HttpError && cause.status === 404) throw new WorktreeMissing();
        throw cause;
      }
    },
    async checkoutFile(pair, id, file, signal) {
      const params = new URLSearchParams({checkout:id, head:file.head, path:file.path});
      if (file.base) params.set('base',file.base);
      if (file.oldPath) params.set('oldPath',file.oldPath);
      return checkoutFile.parse(await json(endpoint(pair.endpoint), '/api/mobile/source-control/file?'+params.toString(), {headers:authorization(pair),signal}));
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
      try {
        return checkoutSessions.parse(await json(endpoint(pair.endpoint), path, { headers: authorization(pair), signal }));
      } catch (cause) {
        if (cause instanceof HttpError && cause.status === 404) throw new WorktreeMissing();
        throw cause;
      }
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
    async open(pair, thread, update, disconnected, checkoutId, signal) {
      const base = endpoint(pair.endpoint);
      const initial = boundedSchema.parse(
        await json(
          base,
          '/api/orchestration/threads/' +
            encodeURIComponent(thread) +
            '/bounded?rcaChatLayout=1',
          {
            signal,
            headers: {
              ...authorization(pair),
              'x-t3-orchestration-protocol': '2',
            },
          }
        ).catch(async cause => {
          if (cause instanceof HttpError && cause.status === 404 && checkoutId) {
            const catalog = await this.workspaces(pair, signal ?? new AbortController().signal);
            if (!catalog.checkouts.some(row => row.id === checkoutId && !row.missing)) throw new WorktreeMissing();
          }
          throw cause;
        })
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
