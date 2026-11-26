import { Logger } from '@lib/logging';
import { randomUUID } from 'expo-crypto';
import AsyncStorage from '@react-native-async-storage/async-storage';
import { z } from 'zod';
import type { Pairing } from '../../domain/mobile';
import { RcaUnavailable } from '../../domain/t3Mobile';
import {
  layoutSchema,
  type WorkspaceTabsPort,
} from '../../domain/workspaceTabs/tabs';
import { endpoint } from '../rca/endpoint';
import { authorization, HttpError, json } from './http';

const note = z.object({ path: z.string(), content: z.string() });
const draft = z.object({ previous: z.string(), content: z.string() });
const draftKey = (pair: Pairing, checkout: string, path: string) =>
  'rca.note.' + JSON.stringify([endpoint(pair.endpoint), checkout, path]);
const headers = (pair: Pairing) => ({
  ...authorization(pair),
  'Content-Type': 'application/json',
});

async function socket(pair: Pairing, kind: 'rpc' | 'media', path: string) {
  const base = endpoint(pair.endpoint);
  const { ticket } = z.object({ ticket: z.string() }).parse(
    await json(base, '/api/auth/websocket-ticket/' + kind, {
      method: 'POST',
      headers: authorization(pair),
    })
  );
  const url = new URL(base.replace(/^http/, 'ws') + path);
  url.searchParams.set('wsTicket', ticket);
  return new WebSocket(url.toString());
}

function connect(
  ws: WebSocket,
  start: () => void,
  failed: (message: string) => void
) {
  let opened = false;
  const timer = setTimeout(() => ws.close(), 15000);
  ws.onopen = () => {
    clearTimeout(timer);
    opened = true;
    start();
  };
  ws.onclose = () => {
    clearTimeout(timer);
    failed(
      opened
        ? 'Connection lost. Reconnect to this tab.'
        : new RcaUnavailable().message
    );
  };
  ws.onerror = () => ws.close();
  return () => {
    clearTimeout(timer);
    ws.onmessage = ws.onopen = ws.onerror = ws.onclose = null;
    ws.close();
  };
}
function send(ws: WebSocket, message: object) {
  if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(message));
}
function imageUri(buffer: ArrayBuffer) {
  const bytes = new Uint8Array(buffer);
  const parts: string[] = [];
  for (let i = 0; i < bytes.length; i += 8192)
    parts.push(String.fromCharCode(...bytes.subarray(i, i + 8192)));
  return 'data:image/jpeg;base64,' + btoa(parts.join(''));
}

async function rpc(
  pair: Pairing,
  tag: string,
  payload: object
): Promise<unknown> {
  const ws = await socket(pair, 'rpc', '/ws?orchestrationProtocol=2');
  return new Promise((resolve, reject) => {
    const finish = () => {
      clearTimeout(timer);
      ws.onclose = ws.onerror = ws.onmessage = null;
      ws.close();
    };
    const fail = () => {
      finish();
      reject(new Error('Could not complete ' + tag + ' on this desktop.'));
    };
    const timer = setTimeout(fail, 15000);
    ws.onerror = ws.onclose = fail;
    ws.onopen = () =>
      send(ws, { _tag: 'Request', id: '1', tag, payload, headers: [] });
    ws.onmessage = ({ data }) => {
      try {
        const raw = JSON.parse(String(data));
        for (const frame of Array.isArray(raw) ? raw : [raw]) {
          if (frame._tag === 'Ping') send(ws, { _tag: 'Pong' });
          if (frame._tag !== 'Exit' || frame.requestId !== '1') continue;
          if (frame.exit?._tag !== 'Success') {
            fail();
            return;
          }
          finish();
          resolve(frame.exit.value);
        }
      } catch (cause) {
        Logger.withError(cause).warn('coding_agent.tab_rpc_invalid');
        fail();
      }
    };
  });
}

import { voiceRecording } from '../../domain/workspaceTabs/voiceRecording';

export function createWorkspaceTabsApi(): WorkspaceTabsPort {
  return {
    async terminalImage(pair, checkout, jpeg) {
      return z.object({ path: z.string() }).parse(
        await json(
          endpoint(pair.endpoint),
          '/api/mobile/terminal-images?checkout=' +
            encodeURIComponent(checkout),
          {
            method: 'POST',
            headers: { ...authorization(pair), 'Content-Type': 'image/jpeg' },
            body: jpeg,
          }
        )
      ).path;
    },
    async transcribe(pair, wav) {
      return z.object({ text: z.string() }).parse(
        await json(endpoint(pair.endpoint), '/api/voice/transcribe', {
          method: 'POST',
          headers: {
            ...authorization(pair),
            'Content-Type': 'application/octet-stream',
          },
          body: voiceRecording(wav),
        })
      ).text;
    },
    async agents(pair) {
      const result = z
        .object({
          providers: z.array(
            z.object({
              instanceId: z.string(),
              enabled: z.boolean().optional(),
              models: z.array(z.object({ slug: z.string() })),
            })
          ),
        })
        .parse(await rpc(pair, 'server.getConfig', {}));
      const names: Record<string, string> = {
        claudeAgent: 'Claude',
        codex: 'Codex',
        opencode: 'OpenCode',
      };
      return result.providers.flatMap((p) =>
        p.enabled !== false && names[p.instanceId] && p.models[0]
          ? [
              {
                id: p.instanceId,
                name: names[p.instanceId],
                model: p.models[0].slug,
              },
            ]
          : []
      );
    },
    async startAgent(pair, layout, agent) {
      const { projects } = z
        .object({
          projects: z.array(
            z.object({ id: z.string(), workspaceRoot: z.string() })
          ),
        })
        .parse(
          await json(endpoint(pair.endpoint), '/api/projects', {
            headers: authorization(pair),
          })
        );
      const known = projects.find(
        (project) => project.workspaceRoot === layout.directory
      );
      const projectId = known?.id ?? 'project-' + layout.threadId;
      if (!known)
        await rpc(pair, 'projects.mutate', {
          type: 'project.create',
          commandId: 'create-' + projectId,
          projectId,
          title: layout.directory.split('/').pop(),
          workspaceRoot: layout.directory,
        });
      const threadId = randomUUID();
      await rpc(pair, 'orchestration.dispatchCommand', {
        type: 'thread.create',
        commandId: randomUUID(),
        threadId,
        projectId,
        title: 'New thread',
        modelSelection: { instanceId: agent.id, model: agent.model },
        createdBy: 'user',
        creationSource: 'mobile',
        runtimeMode: 'approval-required',
        interactionMode: 'default',
        branch: null,
        worktreePath: layout.directory,
      });
      return threadId;
    },
    async layout(pair, checkout, action) {
      return layoutSchema.parse(
        await json(
          endpoint(pair.endpoint),
          '/api/mobile/workspace-tabs?checkout=' + encodeURIComponent(checkout),
          {
            headers: headers(pair),
            method: action ? 'POST' : 'GET',
            body: action && JSON.stringify(action),
          }
        )
      );
    },
    async read(pair, checkout, path) {
      return note.parse(
        await json(
          endpoint(pair.endpoint),
          '/api/mobile/notes?' + new URLSearchParams({ checkout, path }),
          { headers: authorization(pair) }
        )
      );
    },
    async save(pair, checkout, path, change) {
      try {
        return note.parse(
          await json(endpoint(pair.endpoint), '/api/mobile/notes', {
            method: 'PUT',
            headers: headers(pair),
            body: JSON.stringify({ checkout, path, ...change }),
          })
        );
      } catch (error) {
        if (error instanceof HttpError && error.status === 409)
          throw new Error(
            'The desktop file changed. Your draft is saved on this device.'
          );
        throw error;
      }
    },
    async draft(pair, checkout, path) {
      const stored = await AsyncStorage.getItem(draftKey(pair, checkout, path));
      return stored ? draft.parse(JSON.parse(stored)) : null;
    },
    async keepDraft(pair, checkout, path, value) {
      const key = draftKey(pair, checkout, path);
      if (value) await AsyncStorage.setItem(key, JSON.stringify(value));
      else await AsyncStorage.removeItem(key);
    },
    async terminal(pair, layout, tab, output, failed) {
      const ws = await socket(pair, 'rpc', '/ws?orchestrationProtocol=2');
      const target = { threadId: layout.threadId, terminalId: tab.resource };
      let id = 0;
      let openId: string;
      let attached = false;
      const location = {
        cwd: layout.directory,
        worktreePath: layout.directory,
      };
      let size: { cols: number; rows: number } | undefined;
      const call = (tag: string, args: object) => {
        const requestId = String(++id);
        send(ws, {
          _tag: 'Request',
          id: requestId,
          tag,
          payload: { ...target, ...args },
          headers: [],
        });
        return requestId;
      };
      ws.onmessage = ({ data }) => {
        try {
          const payload = JSON.parse(String(data));
          for (const frame of Array.isArray(payload) ? payload : [payload]) {
            if (frame._tag === 'Ping') send(ws, { _tag: 'Pong' });
            if (frame._tag === 'Exit' && frame.exit?._tag === 'Failure')
              failed(
                'The terminal request failed. Reopen the tab to reconnect.'
              );
            if (
              frame._tag === 'Exit' &&
              frame.requestId === openId &&
              frame.exit?._tag === 'Success'
            )
              call('terminal.attach', location);
            if (frame._tag !== 'Chunk') continue;
            for (const event of frame.values ?? []) {
              if (event.type === 'snapshot') {
                attached = event.snapshot.status === 'running';
                output(event.snapshot.history, true);
                if (attached && size) call('terminal.resize', size);
              }
              if (event.type === 'output') output(event.data, false);
              if (event.type === 'exited' || event.type === 'closed') {
                attached = false;
                failed('Shell exited. Reconnect to start a new shell.');
              }
            }
            send(ws, { _tag: 'Ack', requestId: frame.requestId });
          }
        } catch (cause) {
          Logger.withError(cause).warn('coding_agent.terminal_invalid_event');
          failed('Could not read terminal output.');
        }
      };
      const close = connect(
        ws,
        () => {
          openId = call('terminal.open', { ...location, ...size });
        },
        failed
      );
      return {
        close,
        write: (data) => call('terminal.write', { data }),
        resize(cols, rows) {
          if (size?.cols === cols && size.rows === rows) return;
          size = { cols, rows };
          if (attached) call('terminal.resize', size);
        },
      };
    },
    async browser(pair, layout, tab, frame, failed, navigation) {
      const query = new URLSearchParams({
        threadId: layout.threadId,
        tabId: tab.resource,
        navigation: '1',
      });
      const ws = await socket(pair, 'media', '/api/preview-stream/ws?' + query);
      ws.binaryType = 'arraybuffer';
      let size: { width: number; height: number; mobile?: boolean } | undefined;
      const post = (type: string, args: object) => send(ws, { type, ...args });
      ws.onmessage = ({ data }) => {
        if (data instanceof ArrayBuffer) {
          frame(imageUri(data));
          post('ack', {});
        } else if (typeof data === 'string') {
          const event = JSON.parse(data);
          if (event.type === 'navigation')
            navigation?.(event.url, event.canGoBack, event.canGoForward);
        }
      };
      const close = connect(
        ws,
        () => {
          if (size) post('resize', size);
        },
        failed
      );
      return {
        close,
        reload: () => post('reload', {}),
        wheel: (deltaX, deltaY) => post('wheel', { deltaX, deltaY }),
        click(x, y, modifiers, button = 'left') {
          if (modifiers?.length || button === 'right') {
            post('mouse', { action: 'click', x, y, button, modifiers });
            return;
          }
          for (const action of ['down', 'up'])
            post('mouse', { action, x, y, button: 'left' });
        },
        text: (text) => post('text', { text }),
        key: (key) => post('key', { key, action: 'down' }),
        navigate: (url) => post('navigate', { url }),
        history: (delta) => post('history', { delta }),
        resize(width, height, mobile) {
          size = { width, height, mobile };
          post('resize', size);
        },
      };
    },
  };
}
