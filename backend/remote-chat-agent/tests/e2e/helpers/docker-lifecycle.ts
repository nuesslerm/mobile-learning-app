/**
 * E2E-suite lifecycle. Same seam as the integration suite — see
 * `tests/helpers/serviceTarget.ts` for why the container build is opt-in rather
 * than mandatory.
 */
import { execFileSync } from 'node:child_process';
import {
  resolveServiceTarget,
  startTarget,
  stopTarget,
  type RunningTarget,
} from '../../helpers/serviceTarget.js';

const target = resolveServiceTarget(process.env, {
  composeFile: 'docker-compose.e2e.yml',
  containerName: 'remote-chat-agent-e2e',
  composePort: 8789,
});

let running: RunningTarget | undefined;

/** Read inside a test, not at module scope — the port is chosen at start time. */
export function baseUrl(): string {
  return target.baseUrl;
}

export function botSecret(): string {
  if (target.secret) return target.secret;
  return execFileSync('wf', ['secret', 'get', 'CHAT_AGENT_API_SECRET'], { encoding: 'utf8' }).trim();
}

export function usesStubbedAgent(): boolean {
  return target.stubbedAgent;
}

export function targetKind(): string {
  return target.kind;
}

export async function startServices(): Promise<void> {
  // The compose stack runs a real opencode, which needs its provider key; without it every turn fails authentication.
  if (target.kind === 'compose' && !process.env['OPENCODE_GO_API_KEY']) {
    throw new Error('OPENCODE_GO_API_KEY is unset: the compose target needs it (OPENCODE_GO_API_KEY="$(wf secret get OPENCODE_GO_API_KEY)")');
  }
  running = await startTarget(target);
}

export async function stopServices(): Promise<void> {
  if (running) await stopTarget(running);
}
