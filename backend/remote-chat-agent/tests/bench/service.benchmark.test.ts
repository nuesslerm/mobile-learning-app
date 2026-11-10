/**
 * Service benchmark: the same scripted session against whichever implementation `RCA_BACKEND_CMD` names (default: the release build
 * of the Rust service), one fresh process per run. Per run it measures cold start to the first `/health`, resident memory idle, after 300
 * health checks and after each scenario, and the latency of each route (p50 of the calls in the scenario). The medians over the
 * runs are written as JSON to `BENCH_OUT`, so a "pre" record (Node) and a "post" record (Rust) come from identical code.
 *
 * Run: `BENCH_LABEL=pre BENCH_OUT=<file> pnpm --filter @app/remote-chat-agent run bench:service`.
 * Memory is read from `/proc/<pid>/status` (VmRSS now, VmHWM peak), so a command that wraps the service in a shell would
 * measure the shell: name the service binary itself.
 */
import { createServer, type Server } from 'node:http';
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { describe, expect, it } from 'vitest';

import {
  startFakeOpencode,
  type FakeOpencode,
} from '../blackbox/fakeOpencode.js';
import { API_SECRET, SERVICE_BINARY, startService, type Service } from '../blackbox/harness.js';
import { WsClient } from '../blackbox/wsClient.js';

const RUNS = Number(process.env['BENCH_RUNS'] ?? 5);
const CALLS = Number(process.env['BENCH_CALLS'] ?? 10);
const TURNS = Number(process.env['BENCH_TURNS'] ?? 20);
const LABEL = process.env['BENCH_LABEL'] ?? 'unlabelled';
/** A comma list of scenario names to run, for an implementation that has not ported every route yet; empty runs them all. */
const ONLY = (process.env['BENCH_ONLY'] ?? '')
  .split(',')
  .map((s) => s.trim())
  .filter(Boolean);
const OUT = process.env['BENCH_OUT'];
const FIXTURES = join(import.meta.dirname, 'fixtures');

type Mb = { rss: number; peak: number };

function memory(pid: number): Mb {
  const status = readFileSync(`/proc/${pid}/status`, 'utf8');
  const kb = (key: string) =>
    Number(new RegExp(`${key}:\\s+(\\d+) kB`).exec(status)?.[1] ?? 0);
  return { rss: kb('VmRSS') / 1024, peak: kb('VmHWM') / 1024 };
}

// The service limits each caller to 120 requests a minute; a loopback peer is told apart by X-Forwarded-For, so every request gets its own address.
let counter = 0;
const rfetch = (url: string, init: RequestInit = {}): Promise<Response> => {
  const n = counter++;
  return fetch(url, {
    ...init,
    headers: {
      ...(init.headers as Record<string, string> | undefined),
      'x-forwarded-for': `10.${(n >> 16) & 255}.${(n >> 8) & 255}.${n & 255}`,
    },
  });
};

const median = (values: number[]): number => {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2
    ? (sorted[mid] ?? 0)
    : ((sorted[mid - 1] ?? 0) + (sorted[mid] ?? 0)) / 2;
};

async function startFakeStt(): Promise<{
  url: string;
  stop: () => Promise<void>;
}> {
  const server: Server = createServer((req, res) => {
    req.resume();
    req.on('end', () => {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(
        JSON.stringify({
          transcript: 'delete the print statement',
          ms: 3,
          model: 'fake',
          threads: 1,
        })
      );
    });
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  const port = typeof address === 'object' && address ? address.port : 0;
  return {
    url: `http://127.0.0.1:${port}/transcribe`,
    stop: () =>
      new Promise<void>((resolve) => {
        server.closeAllConnections();
        server.close(() => resolve());
      }),
  };
}

const chatReply = (sessionID: string, emit: FakeOpencode['emit']): void => {
  const text = 'Here is the answer. '.repeat(40);
  emit({
    type: 'message.updated',
    properties: {
      sessionID,
      info: { id: `msg-${sessionID}`, role: 'assistant', sessionID },
    },
  });
  for (let i = 0; i < 20; i++) {
    emit({
      type: 'message.part.delta',
      properties: {
        sessionID,
        messageID: `msg-${sessionID}`,
        partID: `part-${sessionID}`,
        field: 'text',
        delta: text.slice(i * 40, i * 40 + 40),
      },
    });
  }
  emit({
    type: 'message.part.updated',
    properties: {
      sessionID,
      part: {
        id: `part-${sessionID}`,
        type: 'text',
        text,
        messageID: `msg-${sessionID}`,
        sessionID,
      },
    },
  });
  emit({ type: 'session.idle', properties: { sessionID } });
};

const tamilReply = (sessionID: string, emit: FakeOpencode['emit']): void => {
  const verdict = JSON.stringify({
    ok: true,
    score: 88,
    summary: 'The right panel reads as the left.',
    fix: '',
  });
  emit({
    type: 'message.part.updated',
    properties: {
      sessionID,
      part: {
        id: `p-${sessionID}`,
        type: 'text',
        text: verdict,
        messageID: `m-${sessionID}`,
        sessionID,
      },
    },
  });
  emit({ type: 'session.idle', properties: { sessionID } });
};

type RunResult = {
  coldStartMs: number;
  idle: Mb;
  afterHealth: Mb;
  healthP50Ms: number;
  scenarios: Record<string, { p50Ms: number; after: Mb }>;
  final: Mb;
};

async function timed<T>(
  fn: () => Promise<T>
): Promise<{ ms: number; value: T }> {
  const started = performance.now();
  const value = await fn();
  return { ms: performance.now() - started, value };
}

async function oneRun(): Promise<RunResult> {
  const oc = await startFakeOpencode();
  const stt = await startFakeStt();
  const forkDir = mkdtempSync(join(tmpdir(), 'rca-bench-fork-'));
  mkdirSync(join(forkDir, 'docs'), { recursive: true });
  writeFileSync(
    join(forkDir, 'docs', 'notes.md'),
    `# Notes\n\n${'Some *emphasis* and `code` in a paragraph.\n\n'.repeat(40)}`
  );
  writeFileSync(
    join(forkDir, 'docs', 'code.ts'),
    readFileSync(join(import.meta.dirname, 'fixtures/large-source.ts.txt'), 'utf8')
  );
  let svc: Service | undefined;
  try {
    const started = performance.now();
    svc = await startService({
      env: {
        OPENCODE_SERVER_URL: oc.url,
        STT_URL: stt.url,
        AGENT_PUBLIC_URL: 'https://bench.example',
        FORK_DIR: forkDir,
        RCA_EXEC_ROOT: forkDir,
        OPENCODE_DEFAULT_MODEL: 'opencode-go/deepseek-v4.1-flash',
        OPENCODE_DISPATCH_RUN_LOG: join(forkDir, 'runs.jsonl'),
        OPENCODE_POOL_LEDGER: join(forkDir, 'pools.json'),
      },
    });
    const first = await rfetch(`${svc.base}/health`);
    expect(first.status).toBe(200);
    const coldStartMs = performance.now() - started;
    await new Promise((r) => setTimeout(r, 1500));
    const idle = memory(svc.pid);

    const health: number[] = [];
    for (let i = 0; i < 300; i++)
      health.push(
        (await timed(() => rfetch(`${svc!.base}/health`).then((r) => r.text())))
          .ms
      );
    const afterHealth = memory(svc.pid);

    const scenarios: RunResult['scenarios'] = {};
    const scenario = async (
      name: string,
      calls: number,
      call: () => Promise<number>
    ): Promise<void> => {
      if (ONLY.length > 0 && !ONLY.includes(name)) return;
      expect(await call(), `${name} warm-up`).toBe(200);
      const latencies: number[] = [];
      for (let i = 0; i < calls; i++) {
        const { ms, value } = await timed(call);
        expect(value, name).toBe(200);
        latencies.push(ms);
      }
      scenarios[name] = { p50Ms: median(latencies), after: memory(svc!.pid) };
    };

    const post = (path: string, init: RequestInit) =>
      rfetch(`${svc!.base}${path}`, { method: 'POST', ...init }).then(
        async (r) => (await r.arrayBuffer(), r.status)
      );
    const form = (file: string, mime: string, name = 'file') => {
      const body = new FormData();
      body.append(
        name,
        new Blob([readFileSync(join(FIXTURES, file))], { type: mime }),
        file
      );
      return body;
    };

    oc.onPrompt(({ sessionID, emit }) => chatReply(sessionID, emit));
    await scenario('run once', TURNS, async () => {
      const res = await rfetch(`${svc!.base}/runs`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'x-bot-secret': API_SECRET },
        body: JSON.stringify({ prompt: 'hello', cwd: forkDir, shape: 'document', models: ['pp/m1'] }),
      });
      const body = (await res.json()) as { ok?: boolean };
      if (body.ok !== true) throw new Error(`run did not succeed: ${JSON.stringify(body)}`);
      return 200;
    });
    await scenario('convert pdf', CALLS, () =>
      post('/api/convert', { body: form('sample.pdf', 'application/pdf') })
    );
    await scenario('convert pdf (chromium)', CALLS, () =>
      post('/api/convert', { body: form('hard.pdf', 'application/pdf') })
    );
    await scenario('convert docx', CALLS, () =>
      post('/api/convert', {
        body: form(
          'sample.docx',
          'application/vnd.openxmlformats-officedocument.wordprocessingml.document'
        ),
      })
    );

    await scenario('file preview (markdown)', CALLS, () =>
      rfetch(`${svc!.base}/api/file/docs/notes.md`).then(
        async (r) => (await r.text(), r.status)
      )
    );
    await scenario('file preview (typescript)', CALLS, () =>
      rfetch(`${svc!.base}/api/file/docs/code.ts`).then(
        async (r) => (await r.text(), r.status)
      )
    );

    oc.onPrompt(({ sessionID, emit }) => tamilReply(sessionID, emit));
    const ink = readFileSync(join(FIXTURES, 'ink.svg'), 'utf8');
    await scenario('tamil check', CALLS, () =>
      post('/api/tamil/check', {
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          characterId: 'a',
          glyph: 'அ',
          transliteration: 'a',
          svg: ink,
        }),
      })
    );
    await scenario('voice transcribe', CALLS, () =>
      post('/api/voice-transcribe', {
        body: form('sample.docx', 'audio/wav', 'audio'),
      })
    );

    // The WebSocket layer: one request over a held socket, and a crowd of sockets each holding a subscription.
    const wsRequest = async (): Promise<void> => {
      const ws = await WsClient.open(svc!);
      await ws.next((f) => f.kind === 'hello');
      await scenario('ws request', TURNS * 5, async () => {
        const res = await ws.request(`r${counter++}`, 'orchestration.listThreads', {});
        return res['ok'] === true ? 200 : 500;
      });
      ws.close();
    };
    if (ONLY.length === 0 || ONLY.includes('ws request')) await wsRequest();
    if (ONLY.length === 0 || ONLY.includes('ws 100 connections')) {
      const held: WsClient[] = [];
      const opened: number[] = [];
      for (let i = 0; i < 100; i++) {
        const { ms, value } = await timed(async () => {
          const ws = await WsClient.open(svc!);
          await ws.next((f) => f.kind === 'hello');
          ws.send({ kind: 'subscribe', id: 'shell', method: 'orchestration.subscribeShell', params: {} });
          await ws.next((f) => f['id'] === 'shell' && (f['item'] as { kind: string }).kind === 'synchronized');
          return ws;
        });
        held.push(value);
        opened.push(ms);
      }
      scenarios['ws 100 connections'] = { p50Ms: median(opened), after: memory(svc.pid) };
      for (const ws of held) ws.close();
    }

    return {
      coldStartMs,
      idle,
      afterHealth,
      healthP50Ms: median(health),
      scenarios,
      final: memory(svc.pid),
    };
  } finally {
    await svc?.stop();
    await stt.stop();
    await oc.stop();
  }
}

describe('remote-chat-agent service benchmark', () => {
  it(
    `runs ${RUNS} fresh-process runs and writes the medians`,
    async () => {
      const runs: RunResult[] = [];
      for (let i = 0; i < RUNS; i++) runs.push(await oneRun());

      const pick = <T>(f: (r: RunResult) => T): T[] => runs.map(f);
      const names = Object.keys(runs[0]?.scenarios ?? {});
      const artifact =
        process.env['BENCH_ARTIFACT'] ?? SERVICE_BINARY;
      const report = {
        label: LABEL,
        date: new Date().toISOString(),
        command: process.env['RCA_BACKEND_CMD'] ?? SERVICE_BINARY,
        runs: RUNS,
        artifactBytes: (() => {
          try {
            return statSync(artifact).size;
          } catch {
            return null;
          }
        })(),
        coldStartMs: median(pick((r) => r.coldStartMs)),
        idleRssMb: median(pick((r) => r.idle.rss)),
        rssAfterHealthMb: median(pick((r) => r.afterHealth.rss)),
        healthP50Ms: median(pick((r) => r.healthP50Ms)),
        scenarios: Object.fromEntries(
          names.map((name) => [
            name,
            {
              p50Ms: median(pick((r) => r.scenarios[name]?.p50Ms ?? 0)),
              rssAfterMb: median(
                pick((r) => r.scenarios[name]?.after.rss ?? 0)
              ),
            },
          ])
        ),
        finalRssMb: median(pick((r) => r.final.rss)),
        peakRssMb: median(pick((r) => r.final.peak)),
        peakRssMaxMb: Math.max(...pick((r) => r.final.peak)),
      };
      if (OUT) {
        mkdirSync(dirname(OUT), { recursive: true });
        writeFileSync(OUT, `${JSON.stringify(report, null, 2)}\n`);
      }
      console.log(JSON.stringify(report, null, 2));
      expect(API_SECRET.length).toBeGreaterThan(0);
    },
    30 * 60 * 1000
  );
});
