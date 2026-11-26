import { composerCatalog, composerFiles } from './composerDiscovery';
import { Logger } from '@lib/logging';
import { z } from 'zod';
import { resumeCommand } from '../../domain/t3Mobile';
import type { T3Conversation, T3Snapshot } from '../../domain/t3Mobile';

export const threadSchema = z.object({
  id: z.string(),
  projectId: z.string(),
  worktreePath: z.string().nullish(),
  providerInstanceId: z.string().nullish(),
  modelSelection: z.object({ instanceId: z.string(), model: z.string() }).nullish().catch(null),
  runtimeMode: z.enum(['approval-required', 'auto-accept-edits', 'auto', 'full-access']).nullish(),
  title: z.string(),
});
const messageSchema = z.object({
  id: z.string(),
  role: z.enum(['user', 'assistant', 'system']),
  createdAt: z.string().optional(),
  runId: z.string().nullish(),
  text: z.string(),
  attachments: z
    .array(
      z.object({
        id: z.string(),
        type: z.enum(['image', 'file']),
        name: z.string(),
        mimeType: z.string(),
      })
    )
    .optional(),
});
function commandInput(input: unknown): unknown {
  if (typeof input !== 'string' || !input.trimStart().startsWith('{'))
    return input;
  try {
    return JSON.parse(input) as unknown;
  } catch {
    Logger.debug('coding_agent.t3.unstructured_tool_input');
    return input;
  }
}
const turnItemSchema = z.object({
  id: z.string(),
  type: z.string(),
  runId: z.string().nullish(),
  messageId: z.string().nullish(),
  name: z.string().nullish(),
  text: z.string().nullish(),
  input: z.unknown().transform(commandInput).optional(),
  output: z.unknown().optional(),
  status: z.string().optional(),
});
const snapshotSchema = z.object({
  thread: threadSchema,
  messages: z.array(messageSchema),
  turnItems: z.array(turnItemSchema).optional(),
});
export const boundedSchema = z.object({
  snapshotSequence: z.number().int().nonnegative(),
  projection: snapshotSchema,
  rcaProjectRoot: z.string().optional(),
  rcaToolTextOffsets: z
    .record(z.string(), z.number().int().nonnegative())
    .optional(),
});

function upsert<T extends { id: string }>(items: T[], item: T): T[] {
  const index = items.findIndex((value) => value.id === item.id);
  if (index < 0) return [...items, item];
  const next = [...items];
  next[index] = item;
  return next;
}
const chunkSchema = z.object({
  kind: z.string(),
  snapshotSequence: z.number().optional(),
  projection: snapshotSchema.optional(),
  sequence: z.number().optional(),
  event: z.object({ type: z.string(), payload: z.unknown() }).optional(),
});
const envelopeSchema = z.object({
  _tag: z.string(),
  requestId: z.string().optional(),
  values: z.array(z.unknown()).optional(),
  exit: z
    .object({ _tag: z.string(), value: z.unknown().optional() })
    .optional(),
});

export function openConversation(
  socket: WebSocket,
  base: string,
  threadId: string,
  update: (value: T3Snapshot) => void,
  disconnected: (error: Error) => void,
  initial?: z.infer<typeof boundedSchema>,
  projectRoot?: (id: string) => Promise<string>
): T3Conversation {
  let snapshot: T3Snapshot | undefined;
  let sequence = -1;
  let closed = false;
  let nextRequest = 1;
  const parts: string[] = [];
  const pending = new Map<
    string,
    { resolve(value: unknown): void; reject(reason: Error): void }
  >();

  function close(error?: Error) {
    if (closed) return;
    closed = true;
    socket.onopen = socket.onclose = socket.onerror = socket.onmessage = null;
    for (const request of pending.values())
      request.reject(error ?? new Error('Chat closed.'));
    pending.clear();
    parts.length = 0;
    socket.close();
    if (error) disconnected(error);
  }
  function request(id: string, tag: string, payload: unknown) {
    socket.send(
      JSON.stringify({ _tag: 'Request', id, tag, payload, headers: [] })
    );
  }
  function call(tag: string, payload: unknown): Promise<unknown> {
    if (closed || !snapshot)
      return Promise.reject(new Error('Waiting for the thread.'));
    const id = String(++nextRequest);
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      try {
        request(id, tag, payload);
      } catch {
        Logger.warn('coding_agent.t3.rpc_send_failed');
        pending.delete(id);
        reject(new Error('Could not reach the chat server.'));
      }
    });
  }
  function apply(raw: unknown) {
    const item = chunkSchema.parse(raw);
    if (
      item.kind === 'snapshot' &&
      item.projection &&
      item.snapshotSequence !== undefined
    ) {
      sequence = item.snapshotSequence;
      snapshot = item.projection;
    } else if (
      item.kind === 'event' &&
      snapshot &&
      item.sequence !== undefined &&
      item.sequence > sequence
    ) {
      sequence = item.sequence;
      if (item.event?.type === 'message.updated') {
        snapshot = {
          ...snapshot,
          messages: upsert(
            snapshot.messages,
            messageSchema.parse(item.event.payload)
          ),
        };
      } else if (item.event?.type === 'turn-item.updated') {
        const row = turnItemSchema.parse(item.event.payload);
        const existing = snapshot.turnItems ?? [];
        let offsets = snapshot.toolTextOffsets;
        if (
          row.type === 'command_execution' &&
          !existing.some((t) => t.id === row.id)
        ) {
          const preceding = [...existing]
            .reverse()
            .find(
              (t) => t.type === 'assistant_message' && t.runId === row.runId
            );
          offsets = { ...offsets, [row.id]: preceding?.text?.length ?? 0 };
        }
        snapshot = {
          ...snapshot,
          turnItems: upsert(existing, row),
          toolTextOffsets: offsets,
        };
      } else if (item.event?.type === 'thread.metadata-updated' ||
                 item.event?.type === 'thread.model-selection-updated' ||
                 item.event?.type === 'thread.runtime-mode-updated') {
        snapshot = {
          ...snapshot,
          thread: threadSchema.parse(item.event.payload),
        };
      } else return;
    } else return;
    update(snapshot);
  }
  socket.onopen = () => {
    if (initial) {
      sequence = initial.snapshotSequence;
      snapshot = {
        ...initial.projection,
        toolTextOffsets: initial.rcaToolTextOffsets,
      };
      update(snapshot);
    }
    request(
      '1',
      'orchestration.subscribeThread',
      initial ? { threadId, afterSequence: sequence } : { threadId }
    );
  };
  socket.onerror = socket.onclose = () =>
    close(new Error('The chat connection closed.'));
  socket.onmessage = ({ data }) => {
    try {
      // Effect RPC may batch envelopes into a single message.
      let text = String(data);
      if (text.startsWith('\u001e')) {
        parts.push(text.slice(1));
        return;
      }
      if (parts.length) {
        text = parts.join('') + text;
        parts.length = 0;
      }
      const decoded: unknown = JSON.parse(text);
      for (const raw of Array.isArray(decoded) ? decoded : [decoded]) {
        const frame = envelopeSchema.parse(raw);
        if (frame._tag === 'Ping')
          socket.send(JSON.stringify({ _tag: 'Pong' }));
        if (frame._tag === 'Chunk' && frame.requestId === '1') {
          for (const value of frame.values ?? []) apply(value);
          socket.send(JSON.stringify({ _tag: 'Ack', requestId: '1' }));
        }
        if (frame._tag !== 'Exit' || !frame.requestId) continue;
        if (frame.requestId === '1') {
          close(new Error('The thread subscription ended.'));
          return;
        }
        const receipt = pending.get(frame.requestId);
        pending.delete(frame.requestId);
        if (frame.exit?._tag === 'Success') receipt?.resolve(frame.exit.value);
        else receipt?.reject(new Error('The server declined the request.'));
      }
    } catch {
      Logger.warn('coding_agent.t3.invalid_chat_update');
      close(new Error('The server returned an unreadable chat update.'));
    }
  };
  async function checkoutPath() {
    const thread = snapshot?.thread;
    if (thread?.worktreePath) return thread.worktreePath;
    if (initial?.rcaProjectRoot && thread?.projectId === initial.projection.thread.projectId) return initial.rcaProjectRoot;
    if (thread && projectRoot) return projectRoot(thread.projectId);
    throw new Error('Waiting for the workspace.');
  }
  return {
    close: () => close(),
    get commands() {
      const thread = snapshot?.thread;
      return thread?.modelSelection?.instanceId || thread?.providerInstanceId
        ? [{ kind: 'command' as const, token: '/model', description: 'Choose the model' }, resumeCommand]
        : [];
    },
    async files(query) {
      return composerFiles(call, await checkoutPath(), query);
    },
    async catalog() {
      const provider = snapshot?.thread.modelSelection?.instanceId ?? snapshot?.thread.providerInstanceId;
      return provider ? composerCatalog(call, await checkoutPath(), provider) : [];
    },
    async models(instanceId) {
      const config = z.object({ providers: z.array(z.object({
        instanceId: z.string(),
        models: z.array(z.object({ slug: z.string(), name: z.string() })),
      })) }).parse(await call('server.getConfig', {}));
      return config.providers.find((provider) => provider.instanceId === instanceId)?.models ?? [];
    },
    async setModel(commandId, instanceId, model) {
      await call('orchestration.dispatchCommand', {
        type: 'thread.model-selection.set', commandId, threadId, modelSelection: { instanceId, model },
      });
    },
    async setRuntimeMode(commandId, runtimeMode) {
      await call('orchestration.dispatchCommand', {
        type: 'thread.runtime-mode.set', commandId, threadId, runtimeMode,
      });
    },
    async openTab(tabId) {
      await call('mobile.session.openTab', { threadId, tabId });
    },
    async send(commandId, text) {
      await call('orchestration.dispatchCommand', {
        type: 'message.dispatch',
        threadId,
        commandId,
        messageId: commandId,
        text,
        createdBy: 'user',
        creationSource: 'mobile',
      });
    },
    async resolveImage(attachment) {
      const asset = z
        .object({ relativeUrl: z.string(), expiresAt: z.number() })
        .parse(
          await call('assets.createUrl', {
            resource: {
              _tag: 'attachment',
              attachmentId: attachment.id,
              fileName: attachment.name,
              mimeType: attachment.mimeType,
              disposition: 'inline',
            },
          })
        );
      const url = new URL(asset.relativeUrl, base);
      if (url.origin !== new URL(base).origin)
        throw new Error('Unexpected image origin.');
      return { uri: url.href, expiresAt: asset.expiresAt };
    },
  };
}
