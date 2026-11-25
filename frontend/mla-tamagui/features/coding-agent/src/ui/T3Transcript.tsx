import { useMemo } from 'react';
import { Text, Markdown } from '@lib/tamagui-design-system';
import { useAgentComponents } from '@lib/agent-ui-contracts';
import { chatTranscript } from '../domain/chatTranscript';
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

export function T3Transcript({
  snapshot,
  session,
}: {
  snapshot: T3Snapshot;
  session?: T3Conversation;
}) {
  const { MessageBubble, ChatToolActivity } = useAgentComponents();
  const rows = useMemo(() => chatTranscript(snapshot), [snapshot]);
  return (
    <>
      {rows.map((row) => {
        if (row.type === 'activity') {
          const working = row.tools.some((t) => t.status === 'running');
          return (
            <ChatToolActivity
              key={row.key}
              working={working}
              summary={toolSentence(row.tools, working)}
              rows={row.tools.map((t) => ({
                id: t.id,
                name: t.name ?? 'Tool',
                input: display(t.input),
                output: display(t.output),
                status: t.status ?? '',
              }))}
            >
              {!!row.images.length && (
                <T3ChatAttachments attachments={row.images} session={session} />
              )}
            </ChatToolActivity>
          );
        }
        const m = row.message;
        if (m.role === 'system') return <Text key={row.key}>{m.text}</Text>;
        return (
          <MessageBubble key={row.key} role={m.role}>
            {!!m.text && <Markdown content={m.text} />}
            {!!m.attachments?.length && (
              <T3ChatAttachments
                attachments={m.attachments}
                session={session}
              />
            )}
          </MessageBubble>
        );
      })}
    </>
  );
}
