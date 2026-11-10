import { test } from 'node:test';
import assert from 'node:assert';
import { parse } from 'yaml';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const composePath = join(__dirname, '..', 'docker-compose.prod.yml');
const compose = parse(readFileSync(composePath, 'utf8'), { merge: true });

const { services, volumes } = compose;

const APP_SERVICES = ['app', 'agent-dind', 'agent-sessions'];

test('exactly one service is privileged and it is agent-dind', () => {
  const privileged = Object.entries(services).filter(([, s]) => s.privileged);
  assert.equal(privileged.length, 1, 'expected exactly one privileged service');
  assert.equal(privileged[0][0], 'agent-dind');
  assert.equal(privileged[0][1].privileged, true);
});

test('app and agent-sessions both carry DOCKER_HOST', () => {
  assert.equal(services.app.environment.DOCKER_HOST, 'unix:///run/dind/docker.sock');
  assert.equal(
    services['agent-sessions'].environment.DOCKER_HOST,
    'unix:///run/dind/docker.sock',
  );
});

test('no service mounts /var/run/docker.sock', () => {
  for (const [name, svc] of Object.entries(services)) {
    for (const vol of svc.volumes ?? []) {
      assert.ok(
        !vol.includes('/var/run/docker.sock'),
        `${name} mounts /var/run/docker.sock`,
      );
    }
  }
});

test('all four container_names are unique', () => {
  const names = ['tailscale', ...APP_SERVICES].map((s) => services[s].container_name);
  assert.equal(new Set(names).size, names.length, 'container_names must be unique');
});

test('workspace is external with name agent-workspace and every app service mounts it at /workspace', () => {
  assert.equal(volumes.workspace.external, true);
  assert.equal(volumes.workspace.name, 'agent-workspace');
  for (const s of APP_SERVICES) {
    assert.ok(
      services[s].volumes.includes('workspace:/workspace'),
      `${s} must mount workspace:/workspace`,
    );
  }
});
test('every role service carries the shared environment, not just the tools container', () => {
  // The split originally left agent-sessions with two variables, which emptied
  // the authorized_keys entrypoint step's identity sources and locked SSH out
  // of the very surface the split exists to keep alive.
  const SHARED = [
    'SSHID_URL',
    'AGENT_SHELL_E2E_SSH_PUBLIC_KEY',
    'OPENCODE_API_KEY',
    'MINIMAX_API_KEY',
    'KIMI_API_KEY',
    'HOST_LABEL',
    'HOME',
  ];
  for (const s of APP_SERVICES) {
    for (const key of SHARED) {
      assert.ok(
        Object.prototype.hasOwnProperty.call(services[s].environment, key),
        `${s} must carry ${key}`,
      );
    }
  }
});

test('each role service declares its own role and the dind socket stays out of the dind container', () => {
  assert.equal(services.app.environment.AGENT_SHELL_ROLE, 'tools');
  assert.equal(services['agent-sessions'].environment.AGENT_SHELL_ROLE, 'sessions');
  assert.equal(services['agent-dind'].environment.AGENT_SHELL_ROLE, 'dind');
  assert.equal(services.app.environment.DOCKER_HOST, 'unix:///run/dind/docker.sock');
  assert.equal(services['agent-sessions'].environment.DOCKER_HOST, 'unix:///run/dind/docker.sock');
  assert.equal(services['agent-dind'].environment.DOCKER_HOST, undefined);
});
