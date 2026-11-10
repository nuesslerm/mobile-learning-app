/**
 * Resolves WHERE the service under test lives, so a suite never hardcodes it.
 *
 * The service is the Rust binary `rca-service` (backend/remote-chat-agent/rs).
 * Three targets, in resolution order:
 *
 *   existing  REMOTE_CHAT_AGENT_BASE_URL is set — talk to an already-running
 *             instance and spawn nothing. This is how a suite runs against the
 *             DEPLOYED agent on the OCI VM (`wf check` sets it to the tailnet
 *             name, or `wf browser tunnel` exports it).
 *   compose   opt in with REMOTE_CHAT_AGENT_TEST_TARGET=compose — build the
 *             production image and run it beside a real opencode, the only
 *             target that exercises the image itself.
 *   process   the default — spawn the release binary (built on demand, or
 *             named by RCA_SERVICE_BIN) against the opencode server at
 *             OPENCODE_SERVER_URL.
 *
 * There is no mock gateway: the far end is always a real opencode, so
 * `stubbedAgent` is false for every target unless the caller says an existing
 * instance is stubbed. A suite must not assert on canned text, only on protocol
 * invariants (SSE framing, status codes).
 */
import { spawn, type ChildProcess } from 'node:child_process';
import { existsSync } from 'node:fs';
import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export type TargetKind = 'existing' | 'process' | 'compose';

export type ServiceTarget = {
  kind: TargetKind;
  baseUrl: string;
  /**
   * True when the agent's LLM dependency is stubbed and therefore emits known
   * canned text. False against a real agent, where only protocol shape holds.
   */
  stubbedAgent: boolean;
  /**
   * The shared WS secret the target's `/ws` upgrade gate accepts. The spawned
   * `process` target always has one (`rca-test-secret` unless the env provides
   * it, so `load()` cannot swap in a vault value the bot does not know); the
   * `existing`/`compose` targets keep whatever secret their env provides.
   */
  secret: string;
  compose?: { file: string; containerName: string };
};

export type ResolveOptions = {
  /** Compose file for the `compose` target, relative to the package root. */
  composeFile: string;
  /** Container name for the `compose` target's forced cleanup. */
  containerName: string;
  /** Host port the compose target publishes on. */
  composePort: number;
};

/** The package root (`backend/remote-chat-agent`), independent of cwd. */
export const PACKAGE_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

/**
 * Pure: env + options in, descriptor out. No IO, so the precedence rules are
 * unit-testable without Docker, a network, or a spawned process.
 */
export function resolveServiceTarget(
  env: Record<string, string | undefined>,
  opts: ResolveOptions
): ServiceTarget {
  const explicitBaseUrl = env['REMOTE_CHAT_AGENT_BASE_URL']?.trim();
  if (explicitBaseUrl) {
    return {
      kind: 'existing',
      baseUrl: explicitBaseUrl.replace(/\/+$/, ''),
      // An already-running instance is assumed real unless the caller says
      // otherwise — asserting canned text against a live agent is the failure
      // this flag exists to prevent, so it must not default to true.
      stubbedAgent: env['REMOTE_CHAT_AGENT_STUBBED'] === 'true',
      secret: env['CHAT_AGENT_API_SECRET'] ?? '',
    };
  }

  if (env['REMOTE_CHAT_AGENT_TEST_TARGET'] === 'compose') {
    return {
      kind: 'compose',
      baseUrl: `http://localhost:${opts.composePort}`,
      stubbedAgent: false,
      secret: env['CHAT_AGENT_API_SECRET'] ?? '',
      compose: { file: opts.composeFile, containerName: opts.containerName },
    };
  }

  return {
    kind: 'process',
    baseUrl: '',
    stubbedAgent: false,
    secret: env['CHAT_AGENT_API_SECRET'] ?? 'rca-test-secret',
  };
}

export async function findFreePort(): Promise<number> {
  return new Promise((resolvePort, reject) => {
    const probe = createServer();
    probe.listen(0, '127.0.0.1', () => {
      const { port } = probe.address() as AddressInfo;
      probe.close((err) => (err ? reject(err) : resolvePort(port)));
    });
  });
}

export type PollConfig = { maxAttempts: number; delayMs: number };
const DEFAULT_POLL: PollConfig = { maxAttempts: 60, delayMs: 1000 };

/** What the last probe saw: a status, or the network error and its cause code (ENOTFOUND, ECONNREFUSED). */
function describeFailure(error: unknown): string {
  if (!(error instanceof Error)) return String(error);
  const cause = error.cause as { code?: string; message?: string } | undefined;
  const why = cause?.code ?? cause?.message;
  return why ? `${error.message} (${why})` : error.message;
}

export async function waitForHealth(
  baseUrl: string,
  poll: PollConfig = DEFAULT_POLL
): Promise<void> {
  let last = 'no probe ran';
  for (let i = 0; i < poll.maxAttempts; i++) {
    try {
      const res = await fetch(`${baseUrl}/health`, { signal: AbortSignal.timeout(5000) });
      if (res.ok) return;
      last = `HTTP ${res.status}`;
    } catch (error) {
      last = describeFailure(error);
    }
    await new Promise((r) => setTimeout(r, poll.delayMs));
  }
  throw new Error(`service at ${baseUrl} did not become healthy in ${poll.maxAttempts} probes (last: ${last})`);
}

/** Handle returned by `startTarget`, passed back to `stopTarget`. */
export type RunningTarget = { target: ServiceTarget; child?: ChildProcess };

async function execShell(command: string, cwd: string): Promise<void> {
  const { exec } = await import('node:child_process');
  const { promisify } = await import('node:util');
  await promisify(exec)(command, { cwd });
}

/** The release binary: `RCA_SERVICE_BIN`, else the workspace build, compiled first when it is not there. */
async function serviceBinary(): Promise<string> {
  const named = process.env['RCA_SERVICE_BIN']?.trim();
  if (named) return named;
  const built = resolve(PACKAGE_ROOT, '../target/release/rca-service');
  if (!existsSync(built)) {
    await execShell('cargo build --release --manifest-path Cargo.toml -p rca-service', PACKAGE_ROOT);
  }
  return built;
}

export async function startTarget(target: ServiceTarget): Promise<RunningTarget> {
  if (target.kind === 'existing') {
    // Inside the suites' 30 s hook timeout (a probe waits up to 5 s), so a service that never answers fails with its own error, not the hook's.
    await waitForHealth(target.baseUrl, { maxAttempts: 4, delayMs: 1000 });
    return { target };
  }

  if (target.kind === 'compose') {
    const { file, containerName } = target.compose!;
    await execShell(`docker rm -f ${containerName} 2>/dev/null || true`, PACKAGE_ROOT).catch(() => {});
    await execShell(`docker compose -f ${file} down -v --remove-orphans 2>/dev/null || true`, PACKAGE_ROOT).catch(() => {});
    await execShell(`docker compose -f ${file} up -d --build --force-recreate`, PACKAGE_ROOT);
    await waitForHealth(target.baseUrl);
    return { target };
  }

  const binary = await serviceBinary();
  const port = await findFreePort();
  const baseUrl = `http://127.0.0.1:${port}`;
  const child = spawn(binary, [], {
    cwd: PACKAGE_ROOT,
    env: {
      ...process.env,
      PORT: String(port),
      // An explicit value here means the service never asks the vault for it,
      // so the /ws upgrade gate matches the `secret` on the descriptor (and
      // thus what the bot client sends as `?token=`).
      CHAT_AGENT_API_SECRET: process.env['CHAT_AGENT_API_SECRET'] ?? 'rca-test-secret',
    },
    stdio: 'pipe',
  });
  // Mutating the descriptor rather than returning a new one keeps the port the
  // single source of truth for callers that captured `target` earlier.
  target.baseUrl = baseUrl;

  try {
    await waitForHealth(baseUrl, { maxAttempts: 45, delayMs: 500 });
  } catch (err) {
    child.kill('SIGTERM');
    throw err;
  }
  return { target, child };
}

export async function stopTarget(running: RunningTarget): Promise<void> {
  const { target, child } = running;

  if (target.kind === 'existing') return; // never touch a service we didn't start

  if (target.kind === 'compose') {
    const { file, containerName } = target.compose!;
    await execShell(`docker compose -f ${file} down -v --remove-orphans`, PACKAGE_ROOT).catch(() => {});
    await execShell(`docker rm -f ${containerName} 2>/dev/null || true`, PACKAGE_ROOT).catch(() => {});
    return;
  }

  if (child && !child.killed) {
    child.kill('SIGTERM');
    await new Promise((r) => setTimeout(r, 200));
    if (!child.killed) child.kill('SIGKILL');
  }
}
