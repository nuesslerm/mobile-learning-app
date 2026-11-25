import type { AgentEvent } from './mobile';

export type ConversationRow = { seq: number; turn?: string; label: string; body: string };
export type Pending = { id: string; kind: 'approvals' | 'questions'; description: string; turn?: string };
export function conversation(events: readonly AgentEvent[]) {
  const rows: ConversationRow[] = [];
  const pending = new Map<string, Pending>();
  const children = new Map<string, { id: string; label: string; status: string }>();
  for (const event of events) {
    const previous = rows[rows.length - 1];
    const add = (label: string, body: string) => rows.push({ seq: event.seq, turn: event.turn, label, body });
    switch (event.type) {
      case 'message.delta':
      case 'reasoning.delta': {
        const label = event.type === 'reasoning.delta' ? 'Thinking' : event.role === 'user' ? 'You' : 'Agent';
        if (previous?.label === label && previous.turn === event.turn) previous.body += event.text ?? '';
        else add(label, event.text ?? '');
        break;
      }
      case 'tool.started': {
        const args = event.args;
        if (event.tool === 'sub_agent_activity' && args && typeof args === 'object' &&
            'agentThreadId' in args && typeof args.agentThreadId === 'string') {
          const child = children.get(args.agentThreadId);
          const label = 'agentPath' in args && typeof args.agentPath === 'string' ? args.agentPath : child?.label ?? args.agentThreadId;
          const status = 'kind' in args && typeof args.kind === 'string' ? args.kind : 'active';
          children.set(args.agentThreadId, { id: args.agentThreadId, label, status });
        } else add('Tool', event.tool ?? 'Running tool');
        break;
      }
      case 'file.changed': add('File changed', event.path ?? ''); break;
      case 'plan.updated': add('Plan', event.plan ?? ''); break;
      case 'error': add('Error', event.message ?? ''); break;
      case 'approval.requested':
        if (event.approval) pending.set(event.approval, { id: event.approval, kind: 'approvals', turn: event.turn,
          description: [event.tool, JSON.stringify(event.args, null, 2)].filter(Boolean).join('\n') });
        break;
      case 'input.requested':
        if (event.request) pending.set(event.request, { id: event.request, kind: 'questions', turn: event.turn, description: event.prompt ?? '' });
        break;
      case 'approval.answered': if (event.approval) pending.delete(event.approval); break;
      case 'input.answered':
      case 'input.dismissed': if (event.request) pending.delete(event.request); break;
      case 'turn.finished':
        for (const [id, value] of pending) if (value.turn === event.turn) pending.delete(id);
        break;
    }
  }
  return { rows, pending: [...pending.values()], children: [...children.values()] };
}
