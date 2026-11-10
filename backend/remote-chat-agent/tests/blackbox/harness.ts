/**
 * The benchmark's harness for remote-chat-agent (tests/bench): the service runs as its own process on a real socket and the bench
 * speaks HTTP to it. The behaviour is pinned by the Rust flow tests (rs/tests/flow.rs). `RCA_BACKEND_CMD` names what to run (default: the release build of the Rust service, built by `global-setup.ts`), so the
 * same suite can run on another implementation without a change to a test.
 */
import { spawn, type ChildProcess } from 'node:child_process';
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { connect, createServer as createTcpServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

export const API_SECRET = 'bot-secret';
/** The release build of the service: the harness's default command. */
export const SERVICE_BINARY = join(import.meta.dirname, '../../../target/release/rca-service');
export const ALLOWED_ORIGIN = 'https://allowed.example';

export type Service = {
  base: string;
  /** The service process (the command's own, so a wrapper script would hide it). */
  pid: number;
  dir: string;
  logs: () => string;
  alive: () => boolean;
  call: (
    path: string,
    init?: {
      method?: string;
      headers?: Record<string, string>;
      body?: string | Buffer;
    }
  ) => Promise<{
    status: number;
    headers: Headers;
    text: string;
    json: () => unknown;
  }>;
  stop: () => Promise<void>;
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

export async function startService(
  opts: { env?: Record<string, string> } = {}
): Promise<Service> {
  const dir = mkdtempSync(join(tmpdir(), 'rca-blackbox-'));
  // The service's vault lookup shells out to `wf`; here every value is already in the environment.
  const noopWf = join(dir, 'wf');
  writeFileSync(noopWf, '#!/bin/sh\nexit 0\n');
  chmodSync(noopWf, 0o755);

  const port = await freePort();
  const [cmd = SERVICE_BINARY, ...args] = (
    process.env['RCA_BACKEND_CMD'] ?? SERVICE_BINARY
  ).split(' ');
  const env: Record<string, string> = {
    PATH: process.env['PATH'] ?? '',
    HOME: dir,
    PORT: String(port),
    AGENT_CLI: 'opencode',
    CHAT_AGENT_API_SECRET: API_SECRET,
    CHAT_AGENT_ALLOWED_ORIGINS: `${ALLOWED_ORIGIN}/`,
    WF_BIN: noopWf,
    GIT_COMMIT: 'blackbox-sha',
    OPENCODE_DB_PATH: join(dir, 'opencode.db'),
    RCA_DEFAULT_CWD: dir,
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

  const deadline = Date.now() + 30_000;
  while (!(await accepts(port))) {
    if (exited || Date.now() > deadline) {
      throw new Error(`service did not start:\n${output}`);
    }
    await new Promise((r) => setTimeout(r, 25));
  }

  const base = `http://127.0.0.1:${port}`;
  return {
    base,
    pid: child.pid ?? 0,
    dir,
    logs: () => output,
    alive: () => !exited,
    call: async (path, init = {}) => {
      const headers: Record<string, string> = { ...(init.headers ?? {}) };
      if (init.body !== undefined && !headers['content-type']) {
        headers['content-type'] = 'application/json';
      }
      const res = await fetch(`${base}${path}`, {
        method: init.method ?? 'GET',
        headers,
        ...(init.body === undefined ? {} : { body: typeof init.body === 'string' ? init.body : new Uint8Array(init.body) }),
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
      rmSync(dir, {
        recursive: true,
        force: true,
        maxRetries: 5,
        retryDelay: 100,
      });
    },
  };
}
