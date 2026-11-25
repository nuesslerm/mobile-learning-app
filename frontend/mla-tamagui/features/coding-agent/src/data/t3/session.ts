import { Logger } from '@lib/logging';
import { z } from 'zod';
import type { T3Conversation, T3Snapshot } from '../../domain/t3Mobile';

export const threadSchema = z.object({
  id: z.string(),
  projectId: z.string(),
  title: z.string(),
});
const messageSchema = z.object({
  id: z.string(),
  role: z.enum(['user', 'assistant', 'system']),
  text: z.string(),
});
const snapshotSchema = z.object({
  thread: threadSchema,
  messages: z.array(messageSchema),
});
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
  exit: z.object({ _tag: z.string() }).optional(),
});

export function openConversation(
  socket: WebSocket,
  threadId: string,
  update: (value: T3Snapshot) => void,
  disconnected: (error: Error) => void
): T3Conversation {
  let snapshot: T3Snapshot | undefined;
  let sequence = -1;
  let closed = false;
  let nextRequest = 1;
  const pending = new Map<
    string,
    { resolve(): void; reject(reason: Error): void }
  >();

  function close(error?: Error) {
    if (closed) return;
    closed = true;
    socket.onopen = socket.onclose = socket.onerror = socket.onmessage = null;
    for (const request of pending.values())
      request.reject(error ?? new Error('Chat closed.'));
    pending.clear();
    socket.close();
    if (error) disconnected(error);
  }

  function request(id: string, tag: string, payload: unknown) {
    socket.send(
      JSON.stringify({ _tag: 'Request', id, tag, payload, headers: [] })
    );
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
        const message = messageSchema.parse(item.event.payload);
        const messages = [...snapshot.messages];
        const index = messages.findIndex((row) => row.id === message.id);
        if (index < 0) messages.push(message);
        else messages[index] = message;
        snapshot = { ...snapshot, messages };
      } else if (item.event?.type === 'thread.metadata-updated') {
        snapshot = {
          ...snapshot,
          thread: threadSchema.parse(item.event.payload),
        };
      } else return;
    } else return;
    update(snapshot);
  }

  socket.onopen = () =>
    request('1', 'orchestration.subscribeThread', { threadId });
  socket.onerror = socket.onclose = () =>
    close(new Error('The chat connection closed.'));
  socket.onmessage = ({ data }) => {
    try {
      // Effect RPC may batch envelopes into a single message.
      const decoded: unknown = JSON.parse(String(data));
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
        if (frame.exit?._tag === 'Success') receipt?.resolve();
        else receipt?.reject(new Error('The server declined the message.'));
      }
    } catch {
      Logger.warn('coding_agent.t3.invalid_chat_update');
      close(new Error('The server returned an unreadable chat update.'));
    }
  };
  return {
    close: () => close(),
    send(commandId, text) {
      if (!snapshot || closed)
        return Promise.reject(new Error('Waiting for the thread.'));
      const id = String(++nextRequest);
      return new Promise<void>((resolve, reject) => {
        pending.set(id, { resolve, reject });
        try {
          request(id, 'orchestration.dispatchCommand', {
            type: 'message.dispatch',
            threadId,
            commandId,
            messageId: commandId,
            text,
            createdBy: 'user',
            creationSource: 'mobile',
          });
        } catch {
          Logger.warn('coding_agent.t3.send_failed');
          pending.delete(id);
          reject(new Error('Could not send the message.'));
        }
      });
    },
  };
}
