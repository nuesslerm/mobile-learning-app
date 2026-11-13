import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { parse } from 'yaml';

const infra = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const load = (app) =>
  parse(readFileSync(join(infra, app, 'docker-compose.prod.yml'), 'utf8'), {
    merge: true,
  });

const shell = load('agent-shell');
const runner = load('ci-runner');
const containers = (c) => Object.values(c.services).map((s) => s.container_name);
const mounts = (c) => Object.values(c.services).flatMap((s) => s.volumes ?? []);

describe('given the agent-shell and ci-runner compose projects', () => {
  it('when agent-shell is redeployed, then no ci-runner container is in its project to be recreated', () => {
    assert.notEqual(shell.name, runner.name);
    const shellContainers = new Set(containers(shell));
    for (const name of containers(runner)) {
      assert.ok(!shellContainers.has(name), `${name} is declared by both projects`);
    }
    assert.equal(
      Object.keys(shell.services).filter((s) => /runner/.test(s)).length,
      0,
      'agent-shell still declares a runner service'
    );
  });

  it('when ci-runner joins the tailnet, then its own key is preferred and the shared key is the fallback', () => {
    assert.equal(
      runner.services.tailscale.environment.TS_AUTHKEY,
      '${TS_CI_RUNNER_AUTH_KEY:-${TS_VM_AUTH_KEY}}'
    );
    assert.equal(runner.services.tailscale.environment.TS_HOSTNAME, 'ci-runner-${HOST_LABEL}');
  });

  it('when ci-runner joins the tailnet, then it is its own node and not the agent-shell one', () => {
    const host = (c) => c.services.tailscale.environment.TS_HOSTNAME;
    assert.equal(host(runner), 'ci-runner-${HOST_LABEL}');
    assert.notEqual(host(runner), host(shell));
    assert.equal(runner.services['gh-runner'].network_mode, 'service:tailscale');
  });

  it('when both projects keep state, then only the external clis volume is shared and the tailnet state is separate', () => {
    const shared = Object.keys(runner.volumes).filter((v) => v in shell.volumes);
    const named = (c, v) => c.volumes[v]?.name ?? `${c.name}_${v}`;
    assert.deepEqual(
      shared.filter((v) => named(runner, v) === named(shell, v)),
      ['clis']
    );
    assert.equal(runner.volumes.clis.external, true);
    assert.equal(shell.volumes.clis.external, true);
    assert.ok(mounts(shell).some((m) => m.startsWith('clis:/opt/clis:ro')));
    assert.ok(mounts(runner).some((m) => m === 'clis:/opt/clis'));
  });

  it('when the runner needs docker, then it reaches no host docker socket and no agent-shell daemon, only its own builder socket', () => {
    for (const m of mounts(runner)) {
      assert.ok(!/docker\.sock|dind-socket/.test(m), `runner mounts ${m}`);
    }
    assert.equal(runner.services['gh-runner'].environment.DOCKER_HOST, 'unix:///run/builder/docker.sock');
  });
});
