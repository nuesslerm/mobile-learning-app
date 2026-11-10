/**
 * Benchmark harness for leetcode-backend: the service runs as its own process on a real socket, with a mock leetcode.com
 * and a mock capture tool beside it, and the benchmark speaks HTTP to it. `LEETCODE_BACKEND_CMD` names what to run (default:
 * the release binary built from `rs/`), so an implementation can be swapped without touching a test.
 */
import { spawn, type ChildProcess } from 'node:child_process';
import {
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { connect, createServer as createTcpServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

export const BEARER = 'gateway-seed';
export const ADMIN = 'admin-secret';
export const CAPTURE_SECRET = 'capture-secret';

export type Recorded = {
  method: string;
  url: string;
  headers: IncomingMessage['headers'];
  body: string;
};

export type Mock = {
  url: string;
  seen: Recorded[];
  /** Replaces the handler for the next requests. */
  reply: (handler: Handler) => void;
  stop: () => Promise<void>;
};

export type Handler = (req: Recorded, res: ServerResponse) => void;

export async function mock(initial: Handler): Promise<Mock> {
  let handler = initial;
  const seen: Recorded[] = [];
  const server = createServer((req, res) => {
    let body = '';
    req.on('data', (c) => (body += String(c)));
    req.on('end', () => {
      const rec: Recorded = {
        method: req.method ?? '',
        url: req.url ?? '',
        headers: req.headers,
        body,
      };
      seen.push(rec);
      handler(rec, res);
    });
  });
  await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
  const port = (server.address() as { port: number }).port;
  return {
    url: `http://127.0.0.1:${port}`,
    seen,
    reply: (h) => {
      handler = h;
    },
    stop: () => new Promise((r) => server.close(() => r())),
  };
}

export const json =
  (status: number, body: unknown): Handler =>
  (_req, res) => {
    res.writeHead(status, { 'content-type': 'application/json' });
    res.end(JSON.stringify(body));
  };

async function freePort(): Promise<number> {
  const s = createTcpServer();
  await new Promise<void>((r) => s.listen(0, '127.0.0.1', r));
  const port = (s.address() as { port: number }).port;
  await new Promise<void>((r) => s.close(() => r()));
  return port;
}

function accepts(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const s = connect(port, '127.0.0.1');
    s.once('connect', () => {
      s.destroy();
      resolve(true);
    });
    s.once('error', () => resolve(false));
  });
}

export type Session = {
  sessionId: string;
  csrfToken: string;
  capturedAt: number;
  lastVerifiedAt: number;
  status: 'active' | 'expired';
};

export type Service = {
  base: string;
  sessionFile: string;
  leetcode: Mock;
  capture: Mock;
  logs: () => string;
  pid: number | undefined;
  /** The service process is still alive. */
  alive: () => boolean;
  setSession: (s: Session | null) => void;
  readSession: () => Session | null;
  /** An authorized (bearer) request unless `headers` says otherwise. */
  call: (
    path: string,
    init?: {
      method?: string;
      headers?: Record<string, string>;
      body?: string;
      bearer?: boolean;
      admin?: boolean;
    }
  ) => Promise<{ status: number; headers: Headers; text: string; json: () => unknown }>;
  stop: () => Promise<void>;
};

export function freshSession(over: Partial<Session> = {}): Session {
  const now = Date.now();
  return {
    sessionId: 'sess-123',
    csrfToken: 'csrf-456',
    capturedAt: now,
    lastVerifiedAt: now,
    status: 'active',
    ...over,
  };
}

export async function startService(
  opts: { env?: Record<string, string> } = {}
): Promise<Service> {
  const dir = mkdtempSync(join(tmpdir(), 'lc-bench-'));
  const sessionFile = join(dir, 'session.json');
  const leetcode = await mock(json(404, { error: 'unmocked' }));
  const capture = await mock((_req, res) => {
    res.writeHead(200);
    res.end('{}');
  });
  const port = await freePort();
  const [cmd = 'node', ...args] = (
    process.env['LEETCODE_BACKEND_CMD'] ??
      join(import.meta.dirname, '../../../target/release/leetcode-backend-rs')
  ).split(' ');
  const env: Record<string, string> = {
    PATH: process.env['PATH'] ?? '',
    HOME: dir,
    PORT: String(port),
    VM_GATEWAY_AUTH_SEED: BEARER,
    LEETCODE_BACKEND_ADMIN_SECRET: ADMIN,
    LEETCODE_CAPTURE_TOOL_BASE_URL: capture.url,
    LEETCODE_CAPTURE_TRIGGER_SECRET: CAPTURE_SECRET,
    LEETCODE_BACKEND_SESSION_FILE: sessionFile,
    LEETCODE_ORIGIN: leetcode.url,
    ...opts.env,
  };
  const child: ChildProcess = spawn(cmd, args, {
    env,
    cwd: join(import.meta.dirname, '../..'),
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let output = '';
  child.stdout?.on('data', (c) => (output += String(c)));
  child.stderr?.on('data', (c) => (output += String(c)));
  let exited = false;
  child.on('exit', () => (exited = true));

  const deadline = Date.now() + 20_000;
  while (!(await accepts(port))) {
    if (exited || Date.now() > deadline) {
      throw new Error(`service did not start:\n${output}`);
    }
    await new Promise((r) => setTimeout(r, 25));
  }

  const base = `http://127.0.0.1:${port}`;
  return {
    base,
    sessionFile,
    leetcode,
    capture,
    logs: () => output,
    pid: child.pid,
    alive: () => !exited,
    setSession: (s) => {
      if (s === null) rmSync(sessionFile, { force: true });
      else writeFileSync(sessionFile, JSON.stringify(s));
    },
    readSession: () => {
      try {
        return JSON.parse(readFileSync(sessionFile, 'utf8')) as Session;
      } catch {
        return null;
      }
    },
    call: async (path, init = {}) => {
      const headers: Record<string, string> = { ...(init.headers ?? {}) };
      if (init.bearer !== false) headers['authorization'] = `Bearer ${BEARER}`;
      if (init.admin) headers['x-admin-secret'] = ADMIN;
      if (init.body !== undefined && !headers['content-type']) {
        headers['content-type'] = 'application/json';
      }
      const res = await fetch(`${base}${path}`, {
        method: init.method ?? 'GET',
        headers,
        ...(init.body === undefined ? {} : { body: init.body }),
      });
      const text = await res.text();
      return {
        status: res.status,
        headers: res.headers,
        text,
        json: () => JSON.parse(text) as unknown,
      };
    },
    stop: async () => {
      child.kill('SIGKILL');
      await leetcode.stop();
      await capture.stop();
      rmSync(dir, { recursive: true, force: true });
    },
  };
}
