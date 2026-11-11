import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { parse } from 'yaml';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', '..');
const compose = parse(readFileSync(join(root, 'infra/oracle-cloud/ci-runner/docker-compose.prod.yml'), 'utf8'), { merge: true });
const groups = JSON.parse(readFileSync(join(root, 'clis/secrets/src/secrets/groups.json'), 'utf8'));
const runner = compose.services['gh-runner'];
const builder = compose.services.builder;
const sources = (service) => (service.volumes ?? []).map((v) => String(v).split(':')[0]);

describe('given the ci-runner compose project with its builder sidecar', () => {
  it('when the runner starts, then no volume mounts the docker socket into any service', () => {
    for (const [name, service] of Object.entries(compose.services)) {
      assert.ok(!sources(service).some((s) => s.includes('docker.sock')), `${name} mounts a docker socket`);
    }
  });

  it('when the runner builds, then its DOCKER_HOST is the builder, over TLS with the client certificates', () => {
    assert.match(String(runner.environment.DOCKER_HOST), /^tcp:\/\/127\.0\.0\.1:2376$/);
    assert.equal(String(runner.environment.DOCKER_TLS_VERIFY), '1');
    assert.equal(runner.environment.DOCKER_CERT_PATH, '/certs/client');
    assert.ok((runner.volumes ?? []).includes('builder-certs:/certs:ro'), 'the runner reads the client certificates, read-only');
    assert.ok(runner.depends_on.includes('builder'));
  });

  it('when the builder runs, then it is TLS-only, in the tailscale network namespace, with its own layer-cache volume and no published port', () => {
    assert.equal(builder.network_mode, 'service:tailscale');
    assert.equal(builder.environment.DOCKER_TLS_CERTDIR, '/certs');
    assert.equal(builder.ports, undefined);
    assert.ok(sources(builder).includes('builder-data') && (builder.volumes ?? []).includes('builder-data:/var/lib/docker'));
    assert.ok(!sources(runner).includes('builder-data'), 'the runner never mounts the daemon state');
    assert.ok(builder.healthcheck, 'the builder has a healthcheck');
    for (const volume of ['builder-data', 'builder-certs', 'gh-runner-config-3']) assert.ok(volume in compose.volumes, `${volume} is declared`);
  });

  it('when the runner registers, then its container env holds a registration token and no PAT', () => {
    assert.equal(runner.environment.ACCESS_TOKEN, undefined);
    assert.match(String(runner.environment.RUNNER_TOKEN), /RUNNER_TOKEN/);
    assert.equal(runner.environment.CONFIGURED_ACTIONS_RUNNER_FILES_DIR, '/runner-config');
    assert.equal(runner.environment.DISABLE_AUTOMATIC_DEREGISTRATION, 'true');
    assert.ok(!JSON.stringify(compose).includes('VM_PROVISION_GITHUB_TOKEN'), 'the compose file never names the PAT');
    const group = groups.find((g) => g.app === 'ci-runner');
    assert.ok(![...group.required, ...group.optional].includes('VM_PROVISION_GITHUB_TOKEN'), 'sync-env writes no PAT into the runner .env');
  });

  it('when a job starts, then the runner holds no vm-ops-api or gateway seed, only the vault read token', () => {
    for (const name of ['VM_OPS_API_AUTH_TOKEN', 'VM_OPS_API_CI_AUTH_TOKEN', 'VM_GATEWAY_AUTH_SEED']) {
      assert.equal(runner.environment[name], undefined, `${name} is not in the runner's environment`);
      assert.ok(!JSON.stringify(compose).includes(`\${${name}`), `${name} is not substituted from the host .env`);
    }
    assert.ok(!('BWS_ADMIN_ACCESS_TOKEN' in runner.environment), 'the runner holds no vault token: jobs get theirs from the workflow');
    const group = groups.find((g) => g.app === 'ci-runner');
    const names = [...group.required, ...group.optional];
    assert.ok(!names.includes('VM_OPS_API_AUTH_TOKEN') && !names.includes('VM_GATEWAY_AUTH_SEED'), 'sync-env writes neither seed into the runner .env');
  });
});
