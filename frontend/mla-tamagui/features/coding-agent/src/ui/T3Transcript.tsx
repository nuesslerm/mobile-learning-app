import { useMemo } from 'react';
import { getNativeChatMessageDay, getNativeChatMessageTimeFormatter } from '../domain/chatMessageTime';
import { useCodingAgentPorts } from '../state/ports';
import { useAgentComponents } from '@lib/agent-ui-contracts';
import { chatTranscript, type ToolGroup } from '../domain/chatTranscript';
import type {
  T3Conversation,
  T3Snapshot,
  T3TurnItem,
} from '../domain/t3Mobile';
import { T3ChatAttachments } from './T3ChatAttachments';

function toolSentence(tools: T3TurnItem[], working: boolean) {
  const phrases = new Map<string, number>();
  for (const tool of tools) {
    const name = tool.name?.toLowerCase() ?? '';
    const kind = ['read', 'read_file', 'view_image'].includes(name)
      ? 'read'
      : ['bash', 'command', 'exec_command', 'run_command', 'shell'].includes(
            name
          )
        ? 'run'
        : 'use';
    phrases.set(kind, (phrases.get(kind) ?? 0) + 1);
  }
  const clauses = [...phrases].map(([kind, count]) => {
    const copy =
      kind === 'read'
        ? [working ? 'reading' : 'read', 'file']
        : kind === 'run'
          ? [working ? 'running' : 'ran', 'command']
          : [working ? 'using' : 'used', 'tool'];
    return copy[0] + ' ' + count + ' ' + copy[1] + (count === 1 ? '' : 's');
  });
  const last = clauses.pop();
  const sentence = clauses.length
    ? clauses.join(', ') + ' and ' + last
    : (last ?? '');
  return sentence.charAt(0).toUpperCase() + sentence.slice(1);
}
const display = (value: unknown) =>
  typeof value === 'string'
    ? value
    : value == null
      ? ''
      : JSON.stringify(value, null, 2);

function Activity({ group, session }: { group: ToolGroup; session?: T3Conversation }) {
  const { ChatToolActivity } = useAgentComponents();
  const working = group.tools.some((tool) => tool.status === 'running');
  return (
    <ChatToolActivity
      working={working}
      summary={toolSentence(group.tools, working)}
      rows={group.tools.map((tool) => ({
        id: tool.id,
        name: tool.name ?? 'Tool',
        input: display(tool.input),
        output: display(tool.output),
        status: tool.status ?? '',
      }))}
    >
      {group.images.length > 0 && <T3ChatAttachments attachments={group.images} session={session} />}
    </ChatToolActivity>
  );
}

export function T3Transcript({ snapshot, session }: { snapshot: T3Snapshot; session?: T3Conversation }) {
  const { copyMessage } = useCodingAgentPorts();
  const { MessageBubble, ChatMessageText, ChatMessageMeta } = useAgentComponents();
  const rows = useMemo(() => chatTranscript(snapshot), [snapshot]);
  const today = getNativeChatMessageDay(Date.now());
  const formatter = getNativeChatMessageTimeFormatter(undefined, today);

  return rows.map((row) => {
    if (row.type === 'activity') {
      return (
        <MessageBubble key={row.key} role="assistant">
          <Activity group={row} session={session} />
        </MessageBubble>
      );
    }
    const message = row.message;
    if (message.role === 'system') return <ChatMessageText key={row.key} allowCodeWrap copyCode={copyMessage} text={message.text} />;
    const date = message.createdAt ? new Date(message.createdAt) : undefined;
    const hasTime = date !== undefined && Number.isFinite(date.getTime());
    const footer = (
      <ChatMessageMeta
        time={hasTime ? formatter.format(date, today.date) : undefined}
        accessibleTime={hasTime ? formatter.formatFull(date) : undefined}
        onCopy={message.text.trim() ? () => copyMessage(message.text) : undefined}
      />
    );
    return (
      <MessageBubble key={row.key} role={message.role} footer={footer}>
        {!!message.attachments?.length && <T3ChatAttachments attachments={message.attachments} session={session} />}
        {!!message.text && <ChatMessageText tone={message.role} allowCodeWrap copyCode={copyMessage} text={message.text} />}
        {row.activity && <Activity group={row.activity} session={session} />}
      </MessageBubble>
    );
  });
}
