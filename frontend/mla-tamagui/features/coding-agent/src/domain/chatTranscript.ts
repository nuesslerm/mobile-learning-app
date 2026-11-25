import type {
  T3Attachment,
  T3Message,
  T3Snapshot,
  T3TurnItem,
} from './t3Mobile';

export type ChatRow =
  | { type: 'message'; key: string; message: T3Message }
  | {
      type: 'activity';
      key: string;
      tools: T3TurnItem[];
      images: T3Attachment[];
    };

function readImage(tool: T3TurnItem, attachments: T3Attachment[]) {
  if (
    !['read', 'read_file', 'view_image'].includes(
      tool.name?.toLowerCase() ?? ''
    )
  )
    return;
  const input = tool.input;
  if (typeof input !== 'object' || !input) return;
  const files = input as Record<string, unknown>;
  const path = files.filePath ?? files.file_path ?? files.path;
  if (typeof path !== 'string') return;
  const name = path.split(/[\\/]/).pop();
  const matches = attachments.filter(
    (a) => a.type === 'image' && a.name === name
  );
  return matches.length === 1
    ? matches[0]
    : matches.find((a) => path.split(/[\\/]/).includes(a.id));
}

export function chatTranscript(snapshot: T3Snapshot): ChatRow[] {
  const rows: ChatRow[] = [];
  const turns = snapshot.turnItems ?? [];
  const runs = new Map<string, T3TurnItem[]>();
  for (const item of turns)
    if (item.runId) {
      const run = runs.get(item.runId);
      if (run) run.push(item);
      else runs.set(item.runId, [item]);
    }
  const byId = new Map(snapshot.messages.map((m) => [m.id, m]));
  const pending = new Map<string | null | undefined, T3Message[]>();
  for (const item of turns) {
    if (item.type !== 'assistant_message' || !item.messageId || !item.text)
      continue;
    const current = byId.get(item.messageId);
    if (current && current.text.length >= item.text.length) continue;
    const updated: T3Message = {
      ...current,
      id: item.messageId,
      runId: item.runId,
      role: 'assistant',
      text: item.text,
    };
    byId.set(updated.id, updated);
    if (!current) {
      const group = pending.get(item.runId);
      if (group) group.push(updated);
      else pending.set(item.runId, [updated]);
    }
  }
  const messages: T3Message[] = [];
  for (const message of snapshot.messages) {
    messages.push(byId.get(message.id) ?? message);
    if (
      message.role === 'user' &&
      message.runId &&
      pending.has(message.runId)
    ) {
      messages.push(...pending.get(message.runId)!);
      pending.delete(message.runId);
    }
  }
  for (const group of pending.values()) messages.push(...group);
  const attachmentsByRun = new Map(
    messages
      .filter((m) => m.role === 'user' && m.runId && m.attachments?.length)
      .map((m) => [m.runId, m.attachments!])
  );
  const answeredRuns = new Set(
    messages.filter((m) => m.role === 'assistant').map((m) => m.runId)
  );
  const shown = new Set<string>();
  for (const message of messages) {
    const runItems = message.runId ? (runs.get(message.runId) ?? []) : [];
    const tools = runItems.filter((t) => t.type === 'command_execution');
    const attachments = attachmentsByRun.get(message.runId) ?? [];
    const images = tools.map((t) => readImage(t, attachments));
    if (message.role !== 'assistant') {
      const viewed = new Set(
        images.filter((a): a is T3Attachment => !!a).map((a) => a.id)
      );
      rows.push({
        type: 'message',
        key: message.id,
        message: {
          ...message,
          attachments: message.attachments?.filter((a) => !viewed.has(a.id)),
        },
      });
      if (!message.runId || answeredRuns.has(message.runId)) continue;
    }
    if (!tools.length || (message.runId && shown.has(message.runId))) {
      if (message.role === 'assistant')
        rows.push({ type: 'message', key: message.id, message });
      continue;
    }
    if (message.runId) shown.add(message.runId);
    const text = message.role === 'assistant' ? message.text : '';
    let start = 0;
    let group: Extract<ChatRow, { type: 'activity' }> | undefined;
    for (const [index, tool] of tools.entries()) {
      const end = Math.max(
        start,
        Math.min(text.length, snapshot.toolTextOffsets?.[tool.id] ?? 0)
      );
      if (end > start) {
        rows.push({
          type: 'message',
          key: message.id + ':' + start,
          message: { ...message, text: text.slice(start, end) },
        });
        start = end;
        group = undefined;
      }
      if (!group) {
        group = { type: 'activity', key: tool.id, tools: [], images: [] };
        rows.push(group);
      }
      group.tools.push(tool);
      const image = images[index];
      if (image && !group.images.some((a) => a.id === image.id))
        group.images.push(image);
    }
    if (start < text.length)
      rows.push({
        type: 'message',
        key: message.id + ':' + start,
        message: { ...message, text: text.slice(start) },
      });
  }
  return rows;
}
