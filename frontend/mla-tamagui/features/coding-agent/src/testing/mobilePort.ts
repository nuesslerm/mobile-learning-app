import type { AgentEvent, MobilePort, MobileThread, Pairing } from '../domain/mobile';

export function createMobileDouble(initial: Pairing | null = null) {
  let pairing = initial;
  const workspace = 'mobile-app';
  const thread: MobileThread = { id: 'thread-42', title: 'Restore editor selection', workspace, status: 'idle' };
  const events: AgentEvent[] = [
    { thread: thread.id, seq: 1, type: 'message.delta', role: 'user', text: 'Check selection' },
    { thread: thread.id, seq: 2, type: 'approval.requested', approval: 'approval-1', tool: 'Read file', args: { path: 'README.md' } },
    { thread: thread.id, seq: 3, type: 'tool.started', tool: 'sub_agent_activity', args: { agentThreadId: 'child', agentPath: '/root/reviewer', kind: 'started' } },
  ];
  let listener: ((e: AgentEvent) => void) | undefined;
  const api: MobilePort = {
    newId: () => 'new-turn',
    readPairing: async () => pairing,
    async pair(endpoint) { pairing = { endpoint, token: 'example' }; return pairing; },
    async revoke() { pairing = null; },
    workspaces: async () => [{ workspace, kind: 'root', attached: true }],
    createWorkspace: async (_pair, _repo, name) => name,
    threads: async () => [thread],
    tasks: async () => [{ number: 42, title: thread.title }],
    createThread: async () => thread.id,
    context: async () => ({ thread, task: { workspace, number: 42 }, family: { threads: [] } }),
    transcript: async () => [{ seq: 1, kind: 'message', role: 'assistant', text: 'Selection restored' }],
    history: async (_pair, _thread, after) => ({ events: events.filter(e => e.seq > after), more: false }),
    follow(_pair, _thread, _after, event) { listener = event; return () => { listener = undefined; }; },
    async send(_pair, id, turn, prompt) {
      listener?.({ seq: 4, thread: id, type: 'message.delta', role: 'user', text: prompt, turn });
    },
    async respond(_pair, id, approval) {
      listener?.({ seq: 4, thread: id, type: 'approval.answered', approval });
    },
  };
  return api;
}
