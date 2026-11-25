import { randomUUID } from 'expo-crypto';
import { Logger } from '@lib/logging';
import type { AgentEvent, MobilePort, MobileThread, Pairing, RepositoryTask, TranscriptItem, Workspace } from '../../domain/mobile';
import { ThreadNotFound } from '../../domain/mobile';
import { readPairing, savePairing } from './credentials';

function endpoint(input: string) {
  const url = new URL(input.trim());
  const local = ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname);
  if ((url.protocol !== 'https:' && !(local && url.protocol === 'http:')) ||
      url.username || url.password || url.search || url.hash) throw new Error('Enter an HTTPS address or a local simulator address.');
  return url.href.replace(/\/$/, '');
}
const wsPath = (id: string) => '/workspaces/' + encodeURIComponent(id);
const threadPath = (id: string) => '/threads/' + encodeURIComponent(id);
async function request<T>(pair: Pairing, path: string, method = 'GET', body?: unknown): Promise<T> {
  const response = await fetch(endpoint(pair.endpoint) + '/v1' + path, {
    method, headers: { 'Content-Type': 'application/json', ...(pair.token ? { Authorization: 'Bearer ' + pair.token } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (method === 'DELETE' && path === '/pair' && response.status === 401) return undefined as T;
  if (!response.ok) {
    const error = await response.text();
    if (response.status === 404 && path.startsWith('/threads/')) throw new ThreadNotFound(error);
    throw new Error(response.status === 401 ? 'Pairing expired or revoked.' : error || 'Gateway request failed.');
  }
  return response.json() as Promise<T>;
}

export function createMobileApi(): MobilePort {
  return {
    newId: randomUUID,
    readPairing,
    async pair(host, code) {
      const url = endpoint(host);
      const { token } = await request<{ token: string }>({ endpoint: url, token: '' }, '/pair', 'POST', { code: code.trim() });
      const pair = { endpoint: url, token };
      await savePairing(pair);
      return pair;
    },
    async revoke(pair) {
      await request(pair, '/pair', 'DELETE');
      await savePairing(null);
    },
    async workspaces(pair) { return (await request<{ workspaces: Workspace[] }>(pair, '/workspaces')).workspaces; },
    async createWorkspace(pair, repo, name, base) {
      return (await request<{ workspace: string }>(pair, '/workspaces', 'POST', { workspace: repo, worktree: name, branch: name, base })).workspace;
    },
    async threads(pair, workspace) {
      return (await request<{ threads: MobileThread[] }>(pair, wsPath(workspace) + '/threads')).threads;
    },
    async tasks(pair, workspace) {
      return (await request<{ issues: RepositoryTask[] }>(pair, wsPath(workspace) + '/tasks')).issues;
    },
    async createThread(pair, workspace, title, driver, task) {
      const response = await request<{ thread: MobileThread }>(pair, wsPath(workspace) + '/threads', 'POST', {
        thread: randomUUID(), title, driver, task,
      });
      return response.thread.id;
    },
    context: (pair, id) => request(pair, threadPath(id)),
    async transcript(pair, id) {
      return (await request<{ items: TranscriptItem[] }>(pair, threadPath(id) + '/transcript')).items;
    },
    history: (pair, id, after) => request(pair, threadPath(id) + '/history?after=' + after),
    follow(pair, id, after, onEvent, closed) {
      const socket = new WebSocket(endpoint(pair.endpoint).replace(/^http/, 'ws') + '/v1' + threadPath(id) + '/events?after=' + after);
      socket.onopen = () => socket.send(JSON.stringify({ token: pair.token }));
      socket.onmessage = ({ data }) => {
        try {
          const { event } = JSON.parse(String(data)) as { event: AgentEvent };
          if (event.thread === id && Number.isSafeInteger(event.seq)) onEvent(event);
        } catch (error) {
          Logger.withError(error).warn('coding_agent.invalid_event');
          socket.close();
        }
      };
      socket.onclose = closed;
      return () => { socket.onclose = null; socket.onmessage = null; socket.onopen = null; socket.close(); };
    },
    async send(pair, thread, id, prompt) { await request(pair, threadPath(thread) + '/turns/' + encodeURIComponent(id), 'POST', { prompt }); },
    async respond(pair, thread, id, kind, value) {
      await request(pair, threadPath(thread) + '/' + kind + '/' + encodeURIComponent(id), 'POST',
        kind === 'approvals' ? { decision: value } : { answer: value });
    },
  };
}
