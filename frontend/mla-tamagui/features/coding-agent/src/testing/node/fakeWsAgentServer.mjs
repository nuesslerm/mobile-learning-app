// features/coding-agent/src/testing/node/fakeWsAgentServer.mjs
//
// A fake orchestration WS agent server for e2e (started on port 8787) and
// local dev. Plain Node ESM, no build step: speaks the wsRpc envelope from
// `@app/remote-chat-agent-contracts` (hello on open, 401 on token mismatch,
// response/event/complete/pong) and replays the deterministic fixtures in
// `node/fixtures.json` (regenerated via export-fixtures.mjs when missing).
//
// Handled RPC: server.probe, orchestration.listThreads / getThread (from an
// in-memory map seeded with fixtures), orchestration.subscribeShell /
// subscribeThread (snapshot then scripted events), orchestration.dispatchCommand
// with scripted turn replies chosen by prompt text `fixture:stream|tool|diff|
// approval|question|plan|fail|slow` (slow = 200 ms per delta for 30 s,
// interruptible). approval / question responses continue the script;
// thread.turn.interrupt stops it. HTTP GET `/__received` returns every
// received command as JSON. `pause()` / `resume()` close and reopen the socket
// server for reconnect tests. CLI entry: `node fakeWsAgentServer.mjs --port 8787`.

import { createServer } from 'node:http';
import { readFileSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Buffer } from 'node:buffer';
import { WebSocketServer } from 'ws';

const here = dirname(fileURLToPath(import.meta.url));
const FIXTURES_PATH = join(here, 'fixtures.json');

const PROTOCOL_VERSION = 1;
const HEARTBEAT_INTERVAL_MS = 15000;
const SLOW_DELTA_MS = 200;
const SLOW_TOTAL_MS = 30000;

function parseScriptedMode(text) {
  const match = /^fixture:(stream|tool|diff|approval|question|plan|fail|slow)\b/.exec(
    text ?? '',
  );
  return match === null ? undefined : match[1];
}

function loadFixtures() {
  if (!existsSync(FIXTURES_PATH)) {
    // fixtures.json is missing -- regenerate it via `tsx`, the only runtime
    // that can import the TypeScript fixture barrel. Fall back to the local
    // `tsx` binary, then to the workspace-root install.
    const scriptPath = join(here, 'export-fixtures.mjs');
    const localTsx = join(here, '..', '..', '..', 'node_modules', '.bin', 'tsx');
    const rootTsx = join(here, '..', '..', '..', '..', '..', '..', '..', 'node_modules', '.bin', 'tsx');
    const tsxBin = existsSync(localTsx) ? localTsx : rootTsx;
    execFileSync(tsxBin, [scriptPath], { stdio: 'inherit' });
  }
  const raw = JSON.parse(readFileSync(FIXTURES_PATH, 'utf8'));
  return { threads: raw.FIXTURE_THREADS, serverConfig: raw.FIXTURE_SERVER_CONFIG };
}

function threadToSummary(thread) {
  const { messages, activities, proposedPlans, ...summary } = thread;
  return summary;
}

function shallowCloneThread(thread) {
  return {
    ...thread,
    modelSelection: thread.modelSelection ? { ...thread.modelSelection } : thread.modelSelection,
    latestTurn: thread.latestTurn ? { ...thread.latestTurn } : thread.latestTurn,
    messages: (thread.messages ?? []).map((m) => ({ ...m })),
    proposedPlans: (thread.proposedPlans ?? []).map((p) => ({ ...p })),
    activities: (thread.activities ?? []).map((a) => ({ ...a })),
    session: thread.session ? { ...thread.session } : thread.session,
  };
}

class FakeWsAgentServer {
  constructor({ port = 8787, token } = {}) {
    this.port = port;
    this.token = token;
    this.received = [];
    this.wss = null;
    this.httpServer = null;
    this.connections = new Set();
    this.subscriptions = new Map();
    this.shellListeners = new Set();
    this.threadListeners = new Map();
    this.pendingApprovals = new Map();
    this.pendingUserInputs = new Map();
    this.activeScript = null;

    const { threads, serverConfig } = loadFixtures();
    this.threads = new Map();
    for (const [id, thread] of Object.entries(threads)) {
      const threadId = `fx-thread-${id}`;
      this.threads.set(threadId, shallowCloneThread({ ...thread, id: threadId }));
    }
    this.serverConfig = serverConfig;
  }

  async start() {
    const httpServer = createServer((req, res) => this.handleHttp(req, res));
    this.httpServer = httpServer;
    await new Promise((resolve, reject) => {
      httpServer.once('error', reject);
      httpServer.listen(this.port, () => resolve());
    });
    this.port = httpServer.address().port;
    this.attachSocketServer();
    return this;
  }

  attachSocketServer() {
    const wss = new WebSocketServer({ noServer: true });
    wss.on('connection', (ws, req) => this.handleConnection(ws, req));
    const onUpgrade = (req, socket, head) => {
      const pathname = this.pathnameOf(req.url);
      if (pathname !== '/ws') return;
      let url;
      try {
        url = new URL(req.url ?? '/', 'http://localhost');
      } catch {
        return;
      }
      const token = url.searchParams.get('token');
      if (this.token !== undefined && token !== this.token) {
        rejectUnauthorized(socket);
        return;
      }
      wss.handleUpgrade(req, socket, head, (ws) => {
        wss.emit('connection', ws, req);
      });
    };
    this.httpServer.on('upgrade', onUpgrade);
    this.wss = wss;
    this.upgradeHandler = onUpgrade;
  }

  get url() {
    return `ws://localhost:${this.port}/ws${this.token ? `?token=${this.token}` : ''}`;
  }

  get httpUrl() {
    return `http://localhost:${this.port}`;
  }

  pause() {
    this.closeSocketServer();
  }

  resume() {
    if (this.wss !== null) return this;
    this.attachSocketServer();
    return this;
  }

  closeSocketServer() {
    this.cancelActiveScript();
    for (const conn of this.connections) {
      try {
        conn.ws.terminate();
      } catch {
        // already closed
      }
    }
    this.connections.clear();
    this.shellListeners.clear();
    this.threadListeners.clear();
    if (this.upgradeHandler !== undefined) {
      this.httpServer?.removeListener('upgrade', this.upgradeHandler);
      this.upgradeHandler = undefined;
    }
    if (this.wss !== null) {
      this.wss.close();
      this.wss = null;
    }
  }

  async stop() {
    this.cancelActiveScript();
    this.closeSocketServer();
    if (this.httpServer !== null) {
      const server = this.httpServer;
      this.httpServer = null;
      await new Promise((resolve) => server.close(() => resolve()));
    }
  }

  handleHttp(req, res) {
    if (req.method === 'GET' && req.url === '/__received') {
      const body = JSON.stringify(this.received);
      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.end(body);
      return;
    }
    res.writeHead(404, { 'Content-Type': 'text/plain' });
    res.end('not found');
  }

  pathnameOf(rawUrl) {
    try {
      return new URL(rawUrl ?? '/', 'http://localhost').pathname;
    } catch {
      return '/';
    }
  }

  handleConnection(ws, _req) {
    const conn = { ws };
    this.connections.add(conn);

    const send = (envelope) => {
      try {
        ws.send(JSON.stringify(envelope));
      } catch {
        // closing
      }
    };

    send({
      kind: 'hello',
      serverVersion: 'fake-ws-agent',
      protocolVersion: PROTOCOL_VERSION,
      heartbeatIntervalMs: HEARTBEAT_INTERVAL_MS,
    });

    let missedPongs = 0;
    const heartbeat = setInterval(() => {
      if (missedPongs >= 2) {
        clearInterval(heartbeat);
        try {
          ws.terminate();
        } catch {
          // already closed
        }
        return;
      }
      missedPongs += 1;
      try {
        ws.ping();
      } catch {
        clearInterval(heartbeat);
      }
    }, HEARTBEAT_INTERVAL_MS);

    ws.on('pong', () => {
      missedPongs = 0;
    });

    ws.on('message', (data) => {
      const text = toText(data);
      if (text === null) return;
      let parsed;
      try {
        parsed = JSON.parse(text);
      } catch {
        send({ kind: 'response', id: '', ok: false, error: { code: 'bad_envelope', message: 'Invalid JSON' } });
        return;
      }
      this.handleEnvelope(parsed, conn, send);
    });

    ws.on('close', () => {
      clearInterval(heartbeat);
      this.connections.delete(conn);
    });

    ws.on('error', () => {
      // ignore
    });
  }

  handleEnvelope(env, conn, send) {
    switch (env.kind) {
      case 'ping':
        send({ kind: 'pong', ts: env.ts });
        return;
      case 'request':
        void this.handleRequest(env.id, env.method, env.params, conn, send);
        return;
      case 'subscribe':
        this.handleSubscribe(env.id, env.method, env.params, conn, send);
        return;
      case 'unsubscribe': {
        const unsub = this.subscriptions.get(env.id);
        if (unsub) {
          unsub();
          this.subscriptions.delete(env.id);
        }
        return;
      }
      default:
        return;
    }
  }

  async handleRequest(id, method, params, conn, send) {
    switch (method) {
      case 'server.probe':
        send({ kind: 'response', id, ok: true, result: this.serverConfig });
        return;
      case 'orchestration.listThreads':
        send({
          kind: 'response',
          id,
          ok: true,
          result: { threads: Array.from(this.threads.values()).map(threadToSummary) },
        });
        return;
      case 'orchestration.getThread': {
        const thread = this.threads.get(params?.threadId);
        if (thread === undefined) {
          send({ kind: 'response', id, ok: false, error: { code: 'thread_not_found', message: 'Thread not found' } });
          return;
        }
        send({ kind: 'response', id, ok: true, result: shallowCloneThread(thread) });
        return;
      }
      case 'orchestration.getTurnDiff':
        send({ kind: 'response', id, ok: true, result: { unifiedDiff: '' } });
        return;
      case 'models.list':
        send({ kind: 'response', id, ok: true, result: { models: [] } });
        return;
      case 'orchestration.dispatchCommand':
        await this.dispatchCommand(params, conn, send, id);
        return;
      default:
        send({ kind: 'response', id, ok: false, error: { code: 'unknown_method', message: 'Unknown method' } });
        return;
    }
  }

  handleSubscribe(id, method, params, conn, send) {
    this.subscriptions.get(id)?.();
    this.subscriptions.delete(id);
    switch (method) {
      case 'orchestration.subscribeShell': {
        const unsub = this.subscribeShell(id, conn, send);
        this.subscriptions.set(id, unsub);
        return;
      }
      case 'orchestration.subscribeThread': {
        const unsub = this.subscribeThread(id, params?.threadId, params?.afterSequence, conn, send);
        this.subscriptions.set(id, unsub);
        return;
      }
      default:
        send({
          kind: 'response',
          id,
          ok: false,
          error: { code: 'unknown_subscription', message: `Unknown subscription: ${method}` },
        });
        return;
    }
  }

  subscribeShell(subId, conn, send) {
    const listener = { conn, send, alive: true };
    this.shellListeners.add(listener);
    const summaries = Array.from(this.threads.values()).map(threadToSummary);
    queueMicrotask(() => {
      if (!listener.alive) return;
      listener.send({ kind: 'event', id: subId, item: { kind: 'synchronized' } });
      listener.send({ kind: 'event', id: subId, item: { kind: 'snapshot', snapshot: summaries } });
    });
    return () => {
      listener.alive = false;
      this.shellListeners.delete(listener);
    };
  }

  subscribeThread(subId, threadId, afterSequence, conn, send) {
    const listener = { subId, conn, send, alive: true };
    const list = this.threadListeners.get(threadId) ?? new Set();
    list.add(listener);
    this.threadListeners.set(threadId, list);

    const thread = this.threads.get(threadId);
    queueMicrotask(() => {
      if (!listener.alive) return;
      listener.send({ kind: 'event', id: subId, item: { kind: 'synchronized' } });
      if (thread !== undefined) {
        listener.send({ kind: 'event', id: subId, item: { kind: 'snapshot', snapshot: shallowCloneThread(thread) } });
      }
    });
    return () => {
      listener.alive = false;
      const current = this.threadListeners.get(threadId);
      if (current) {
        current.delete(listener);
        if (current.size === 0) this.threadListeners.delete(threadId);
      }
    };
  }

  emitToThread(threadId, item) {
    const listeners = this.threadListeners?.get(threadId);
    if (!listeners) return;
    for (const listener of listeners) {
      if (listener.alive) listener.send({ kind: 'event', id: listener.subId, item });
    }
  }

  recordReceived(cmd) {
    this.received.push(cmd);
  }

  async dispatchCommand(cmd, conn, send, id) {
    if (!cmd || typeof cmd !== 'object' || !cmd.type) {
      send({ kind: 'response', id, ok: false, error: { code: 'invalid_params', message: 'Invalid command' } });
      return;
    }
    this.recordReceived(cmd);
    const { commandId } = cmd;

    switch (cmd.type) {
      case 'thread.turn.start':
        await this.dispatchTurnStart(cmd, conn, send, id);
        return;
      case 'thread.approval.respond':
        this.dispatchApprovalRespond(cmd, send, id);
        return;
      case 'thread.user-input.respond':
        this.dispatchUserInputRespond(cmd, send, id);
        return;
      case 'thread.turn.interrupt':
        this.dispatchInterrupt(cmd, send, id);
        return;
      default:
        send({ kind: 'response', id, ok: true, result: { commandId, status: 'accepted', events: [] } });
        return;
    }
  }

  async dispatchTurnStart(cmd, conn, send, id) {
    const thread = this.threads.get(cmd.threadId);
    if (thread === undefined) {
      send({ kind: 'response', id, ok: false, error: { code: 'thread_not_found', message: 'Thread not found' } });
      return;
    }
    const createdAt = new Date().toISOString();
    const messageEvent = {
      eventId: `fx-ev-${Date.now()}-u`,
      sequence: this.nextSequenceFor(thread),
      createdAt,
      threadId: cmd.threadId,
      commandId: cmd.commandId,
      actor: 'client',
      type: 'thread.message-sent',
      payload: {
        message: {
          id: cmd.message?.messageId ?? `fx-msg-${Date.now()}`,
          role: 'user',
          text: cmd.message?.text ?? '',
          turnId: thread.latestTurn?.turnId,
          createdAt,
          updatedAt: createdAt,
        },
      },
    };
    const turnStartEvent = {
      eventId: `fx-ev-${Date.now()}-t`,
      sequence: this.nextSequenceFor(thread),
      createdAt,
      threadId: cmd.threadId,
      commandId: cmd.commandId,
      actor: 'client',
      type: 'thread.turn-start-requested',
      payload: {
        messageId: cmd.message?.messageId,
        runtimeMode: cmd.runtimeMode,
        interactionMode: cmd.interactionMode,
        createdAt,
      },
    };
    this.applyEvent(thread, messageEvent);
    this.applyEvent(thread, turnStartEvent);
    this.emitToThread(cmd.threadId, { kind: 'event', event: messageEvent });
    this.emitToThread(cmd.threadId, { kind: 'event', event: turnStartEvent });

    send({
      kind: 'response',
      id,
      ok: true,
      result: {
        commandId: cmd.commandId,
        status: 'accepted',
        events: [messageEvent, turnStartEvent],
      },
    });

    const mode = parseScriptedMode(cmd.message?.text);
    if (mode !== undefined) {
      this.runScript(mode, cmd, thread);
    }
  }

  dispatchApprovalRespond(cmd, send, id) {
    const pending = this.pendingApprovals.get(cmd.requestId);
    if (!pending) {
      send({ kind: 'response', id, ok: false, error: { code: 'request_not_pending', message: 'Request not pending' } });
      return;
    }
    this.pendingApprovals.delete(cmd.requestId);
    const createdAt = new Date().toISOString();
    const thread = this.threads.get(cmd.threadId);
    this.applyActivity(thread, {
      tone: 'info',
      kind: 'request.resolved',
      summary: 'Approval resolved',
      payload: { requestType: pending.requestType, decision: cmd.decision },
      turnId: pending.turnId,
      createdAt,
    });
    send({ kind: 'response', id, ok: true, result: { commandId: cmd.commandId, status: 'accepted', events: [] } });
    this.continueScriptAfterInterrupt(cmd.threadId);
  }

  dispatchUserInputRespond(cmd, send, id) {
    const pending = this.pendingUserInputs.get(cmd.requestId);
    if (!pending) {
      send({ kind: 'response', id, ok: false, error: { code: 'request_not_pending', message: 'Request not pending' } });
      return;
    }
    this.pendingUserInputs.delete(cmd.requestId);
    const createdAt = new Date().toISOString();
    const thread = this.threads.get(cmd.threadId);
    this.applyActivity(thread, {
      tone: 'info',
      kind: 'user-input.resolved',
      summary: 'User input received',
      payload: { answers: cmd.answers },
      turnId: pending.turnId,
      createdAt,
    });
    send({ kind: 'response', id, ok: true, result: { commandId: cmd.commandId, status: 'accepted', events: [] } });
    this.continueScriptAfterInterrupt(cmd.threadId);
  }

  dispatchInterrupt(cmd, send, id) {
    this.cancelActiveScript();
    send({ kind: 'response', id, ok: true, result: { commandId: cmd.commandId, status: 'accepted', events: [] } });
  }

  continueScriptAfterInterrupt(threadId) {
    if (this.activeScript === null || this.activeScript.state !== 'paused') return;
    const script = this.activeScript;
    if (script.threadId !== threadId) return;
    const message =
      script.mode === 'approval'
        ? 'Approval recorded. Continuing.'
        : 'Got it. Continuing with your answers.';
    this.sendAssistantMessage(script, message);
    script.cancelled = true;
    this.activeScript = null;
  }

  cancelActiveScript() {
    if (this.activeScript !== null) {
      this.activeScript.cancelled = true;
      if (this.activeScript.timer !== null) {
        clearTimeout(this.activeScript.timer);
      }
      this.activeScript = null;
    }
  }

  runScript(mode, cmd, thread) {
    this.cancelActiveScript();
    const script = {
      mode,
      threadId: cmd.threadId,
      thread,
      state: 'running',
      cancelled: false,
      timer: null,
      seq: 0,
      text: '',
      startedAt: Date.now(),
      turnId: `fx-turn-${cmd.threadId}-${Date.now()}`,
    };
    this.activeScript = script;
    this.advanceScript(script);
  }

  advanceScript(script) {
    if (script.cancelled || script.state !== 'running') return;
    switch (script.mode) {
      case 'stream':
        this.streamStep(script);
        return;
      case 'slow':
        this.slowStep(script);
        return;
      case 'tool':
      case 'diff':
      case 'plan':
      case 'fail':
        this.oneShotStep(script);
        return;
      case 'approval':
        this.approvalStep(script);
        return;
      case 'question':
        this.questionStep(script);
        return;
      default:
        return;
    }
  }

  streamStep(script) {
    if (script.cancelled) return;
    const deltas = [
      'Types flow through the code,\ngentle and strict,',
      'Types flow through the code,\ngentle and strict,\neach line a quiet promise.',
    ];
    if (script.seq >= deltas.length) {
      script.cancelled = true;
      this.activeScript = null;
      return;
    }
    const partial = deltas[script.seq];
    script.seq += 1;
    this.sendAssistantMessage(script, partial);
    if (script.seq >= deltas.length) {
      script.cancelled = true;
      this.activeScript = null;
      return;
    }
    script.timer = setTimeout(() => this.streamStep(script), 400);
  }

  slowStep(script) {
    if (script.cancelled) return;
    if (Date.now() - script.startedAt >= SLOW_TOTAL_MS) {
      script.cancelled = true;
      this.activeScript = null;
      return;
    }
    script.text = `${script.text ?? ''} Slow streaming delta ${script.seq}.`;
    script.seq += 1;
    this.sendAssistantMessage(script, script.text);
    script.timer = setTimeout(() => this.slowStep(script), SLOW_DELTA_MS);
  }

  oneShotStep(script) {
    if (script.cancelled) return;
    const thread = script.thread;
    const createdAt = new Date().toISOString();
    if (script.mode === 'tool') {
      const toolActivities = [
        { tone: 'tool', kind: 'tool.completed', summary: 'pnpm install', payload: { itemType: 'command_execution', command: 'pnpm install', status: 'completed' } },
        { tone: 'tool', kind: 'tool.completed', summary: 'pnpm typecheck', payload: { itemType: 'command_execution', command: 'pnpm typecheck', status: 'completed' } },
        { tone: 'tool', kind: 'tool.completed', summary: 'pnpm lint', payload: { itemType: 'command_execution', command: 'pnpm lint', status: 'completed' } },
        { tone: 'tool', kind: 'tool.completed', summary: 'pnpm test:unit', payload: { itemType: 'command_execution', command: 'pnpm test:unit', status: 'completed' } },
        { tone: 'error', kind: 'tool.failed', summary: 'pnpm test:e2e', payload: { itemType: 'command_execution', command: 'pnpm test:e2e', status: 'failed' } },
        { tone: 'error', kind: 'tool.declined', summary: 'pnpm publish', payload: { itemType: 'command_execution', command: 'pnpm publish', status: 'declined' } },
        { tone: 'tool', kind: 'tool.completed', summary: 'Edit src/foo.ts', payload: { itemType: 'file_change', status: 'completed', data: { changedFiles: ['src/foo.ts'] } } },
        { tone: 'tool', kind: 'tool.completed', summary: 'Edit src/foo.test.ts', payload: { itemType: 'file_change', status: 'completed', data: { changedFiles: ['src/foo.test.ts'] } } },
      ];
      for (const activity of toolActivities) {
        this.applyActivity(thread, { ...activity, turnId: script.turnId, createdAt });
      }
      this.sendAssistantMessage(script, 'Patched src/foo.ts and re-ran the suite.');
    } else if (script.mode === 'diff') {
      const unifiedDiff =
        'diff --git a/src/foo.ts b/src/foo.ts\n' +
        '--- a/src/foo.ts\n' +
        '+++ b/src/foo.ts\n' +
        '@@ -1,1 +1,1 @@\n' +
        '-export const a = 1;\n' +
        '+export const a = 2;\n';
      const event = {
        eventId: `fx-ev-${Date.now()}-d`,
        sequence: this.nextSequenceFor(thread),
        createdAt,
        threadId: script.threadId,
        actor: 'provider',
        type: 'thread.turn-diff-updated',
        payload: { turnId: script.turnId, unifiedDiff },
      };
      this.applyEvent(thread, event);
      this.emitToThread(script.threadId, { kind: 'event', event });
      this.sendAssistantMessage(script, 'Updated the constant. Diff is shown above.');
    } else if (script.mode === 'plan') {
      const plan = {
        id: `fx-plan-${script.threadId}-${Date.now()}`,
        turnId: script.turnId,
        planMarkdown:
          '# Fixture plan\n\n' +
          '- [ ] Inspect the change.\n' +
          '- [ ] Apply the fix.\n' +
          '- [ ] Verify the result.\n',
        createdAt,
        updatedAt: createdAt,
      };
      const event = {
        eventId: `fx-ev-${Date.now()}-p`,
        sequence: this.nextSequenceFor(thread),
        createdAt,
        threadId: script.threadId,
        actor: 'provider',
        type: 'thread.proposed-plan-upserted',
        payload: { proposedPlan: plan },
      };
      this.applyEvent(thread, event);
      this.emitToThread(script.threadId, { kind: 'event', event });
      this.sendAssistantMessage(script, 'Here is the plan — review and approve.');
    } else if (script.mode === 'fail') {
      this.applyActivity(thread, {
        tone: 'error',
        kind: 'runtime.error',
        summary: 'Provider error',
        payload: { message: 'Provider returned 500 while streaming the response.', class: 'provider_error' },
        turnId: script.turnId,
        createdAt,
      });
    }
    script.cancelled = true;
    this.activeScript = null;
  }

  approvalStep(script) {
    if (script.cancelled) return;
    const thread = script.thread;
    const createdAt = new Date().toISOString();
    const requestId = `fx-req-${script.threadId}-approval`;
    this.applyActivity(thread, {
      tone: 'approval',
      kind: 'request.opened',
      summary: 'Approve patch',
      payload: { requestId, requestType: 'file_change_approval', detail: 'src/foo.ts' },
      turnId: script.turnId,
      createdAt,
    });
    this.pendingApprovals.set(requestId, { requestId, requestType: 'file_change_approval', turnId: script.turnId, threadId: script.threadId });
    script.state = 'paused';
    this.activeScript = script;
  }

  questionStep(script) {
    if (script.cancelled) return;
    const thread = script.thread;
    const createdAt = new Date().toISOString();
    const requestId = `fx-req-${script.threadId}-user-input`;
    this.applyActivity(thread, {
      tone: 'approval',
      kind: 'user-input.requested',
      summary: 'Need input',
      payload: {
        requestId,
        questions: [
          {
            id: 'q-continue',
            header: 'Continue',
            question: 'Should I keep going?',
            options: [
              { label: 'Yes', description: 'Continue with the plan.' },
              { label: 'No', description: 'Stop here.' },
            ],
            multiSelect: false,
          },
        ],
      },
      turnId: script.turnId,
      createdAt,
    });
    this.pendingUserInputs.set(requestId, { requestId, turnId: script.turnId, threadId: script.threadId });
    script.state = 'paused';
    this.activeScript = script;
  }

  sendAssistantMessage(script, text) {
    const thread = script.thread;
    const createdAt = new Date().toISOString();
    const messageId = `fx-final-${script.threadId}-${Date.now()}-${script.seq}`;
    const event = {
      eventId: `fx-ev-${Date.now()}-m`,
      sequence: this.nextSequenceFor(thread),
      createdAt,
      threadId: script.threadId,
      actor: 'provider',
      type: 'thread.message-sent',
      payload: {
        message: {
          id: messageId,
          role: 'assistant',
          text,
          turnId: script.turnId,
          createdAt,
          updatedAt: createdAt,
        },
      },
    };
    this.applyEvent(thread, event);
    this.emitToThread(script.threadId, { kind: 'event', event });
  }

  applyActivity(thread, activity) {
    const event = {
      eventId: `fx-ev-${Date.now()}-a${Math.random().toString(36).slice(2)}`,
      sequence: this.nextSequenceFor(thread),
      createdAt: activity.createdAt ?? new Date().toISOString(),
      threadId: thread.id,
      actor: 'system',
      type: 'thread.activity-appended',
      payload: {
        activity: {
          // OrchestrationThreadActivity requires an id; scripted activities
          // are created without one, and a missing id makes the client's
          // feed sort crash when two entries tie on (createdAt, sequence).
          id:
            activity.id ??
            `fx-act-${thread.id}-${Date.now()}-${Math.random().toString(36).slice(2)}`,
          ...activity,
        },
      },
    };
    this.applyEvent(thread, event);
    this.emitToThread(thread.id, { kind: 'event', event });
  }

  applyEvent(thread, event) {
    if (event.type === 'thread.message-sent') {
      const msg = event.payload.message;
      thread.messages = [...thread.messages, msg];
    } else if (event.type === 'thread.activity-appended') {
      thread.activities = [...thread.activities, event.payload.activity];
    } else if (event.type === 'thread.proposed-plan-upserted') {
      const plan = event.payload.proposedPlan;
      const existing = thread.proposedPlans.findIndex((p) => p.id === plan.id);
      if (existing >= 0) {
        thread.proposedPlans = thread.proposedPlans.map((p) => (p.id === plan.id ? plan : p));
      } else {
        thread.proposedPlans = [...thread.proposedPlans, plan];
      }
    }
    thread.updatedAt = event.createdAt;
    const seq = event.sequence;
    if (typeof seq === 'number' && seq >= (thread._seq ?? 0)) {
      thread._seq = seq;
    }
  }

  nextSequenceFor(thread) {
    const current = thread._seq ?? 0;
    thread._seq = current + 1;
    return thread._seq;
  }
}

function toText(data) {
  if (typeof data === 'string') return data;
  if (Buffer.isBuffer(data)) return data.toString('utf8');
  if (Array.isArray(data)) return Buffer.concat(data).toString('utf8');
  if (data instanceof ArrayBuffer) return Buffer.from(data).toString('utf8');
  return null;
}

function rejectUnauthorized(socket) {
  const body = 'unauthorized';
  const head =
    'HTTP/1.1 401 Unauthorized\r\n' +
    'Content-Type: text/plain; charset=utf-8\r\n' +
    `Content-Length: ${Buffer.byteLength(body)}\r\n` +
    'Connection: close\r\n' +
    '\r\n';
  socket.write(head);
  socket.write(body);
  socket.destroy();
}

export async function startFakeWsAgentServer(options = {}) {
  const server = new FakeWsAgentServer(options);
  await server.start();
  return server;
}

const isMain = process.argv[1] !== undefined && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
  let port = 8787;
  const argIndex = process.argv.indexOf('--port');
  if (argIndex !== -1 && process.argv[argIndex + 1] !== undefined) {
    const parsed = Number(process.argv[argIndex + 1]);
    if (Number.isFinite(parsed)) port = parsed;
  }
  const server = new FakeWsAgentServer({ port });
  await server.start();
  process.stdout.write(`fake ws agent listening on ${server.httpUrl}\n`);
  const shutdown = async () => {
    await server.stop();
    process.exit(0);
  };
  process.on('SIGINT', () => { shutdown().catch(() => process.exit(1)); });
  process.on('SIGTERM', () => { shutdown().catch(() => process.exit(1)); });
}
