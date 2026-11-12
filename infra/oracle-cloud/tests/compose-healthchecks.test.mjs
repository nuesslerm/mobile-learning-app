import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join, relative } from 'node:path';
import { parse } from 'yaml';
import { loadCompose } from './compose-load.mjs';

const repoRoot = join(
  dirname(fileURLToPath(import.meta.url)),
  '..',
  '..',
  '..'
);

// Production = everything the VM runs: every infra/oracle-cloud compose file plus each backend's prod/capture file.
function productionComposeFiles() {
  const infra = readdirSync(join(repoRoot, 'infra/oracle-cloud'), {
    recursive: true,
  })
    .filter(
      (p) =>
        !p.includes('node_modules') && /(^|\/)docker-compose[^/]*\.yml$/.test(p)
    )
    .map((p) => join(repoRoot, 'infra/oracle-cloud', p));
  const backend = readdirSync(join(repoRoot, 'backend'), {
    withFileTypes: true,
  })
    .filter((d) => d.isDirectory())
    .flatMap((d) =>
      readdirSync(join(repoRoot, 'backend', d.name))
        .filter((f) => /^docker-compose\.(prod|capture)\.yml$/.test(f))
        .map((f) => join(repoRoot, 'backend', d.name, f))
    );
  return [...infra, ...backend].sort();
}

function loadServices(file) {
  return loadCompose(file).services ?? {};
}

const files = productionComposeFiles();

// A service without a healthcheck needs a reason here; the test fails once it gains one or is gone, so no entry goes stale.
const NO_HEALTHCHECK = {};

describe('given every production compose file', () => {
  it('when the files are collected, then the known fleet is all there', () => {
    const rel = files.map((f) => relative(repoRoot, f));
    for (const expected of [
      'infra/oracle-cloud/agent-shell/docker-compose.prod.yml',
      'infra/oracle-cloud/ci-runner/docker-compose.prod.yml',
      'infra/oracle-cloud/vm-ops-api/docker-compose.yml',
      'backend/leetcode-auth-cookie-capture/docker-compose.capture.yml',
      'backend/leetcode-backend/docker-compose.prod.yml',
      'backend/remote-chat-agent/docker-compose.prod.yml',
      'backend/reporting-service/docker-compose.prod.yml',
    ]) {
      assert.ok(rel.includes(expected), `missing ${expected}`);
    }
  });

  for (const file of files) {
    const rel = relative(repoRoot, file);
    for (const [name, svc] of Object.entries(loadServices(file))) {
      it(`when ${rel} declares ${name}, then it carries a healthcheck with a test and bounded timing`, () => {
        const key = `${rel}:${name}`;
        if (key in NO_HEALTHCHECK) {
          assert.equal(
            svc.healthcheck,
            undefined,
            `${key} gained a healthcheck: drop its NO_HEALTHCHECK entry`
          );
          return;
        }
        const hc = svc.healthcheck;
        assert.ok(hc, `${rel}:${name} has no healthcheck`);
        assert.notEqual(
          hc.disable,
          true,
          `${rel}:${name} disables its healthcheck`
        );
        assert.ok(
          Array.isArray(hc.test) && hc.test.length > 1,
          `${rel}:${name} healthcheck.test must be a CMD array`
        );
        for (const key of ['interval', 'timeout', 'retries', 'start_period']) {
          assert.ok(
            hc[key] !== undefined,
            `${rel}:${name} healthcheck.${key} missing`
          );
        }
      });
    }
  }

  it('when a service is excused from a healthcheck, then it is still declared and says why', () => {
    const declared = new Set(
      files.flatMap((f) =>
        Object.keys(loadServices(f)).map((n) => `${relative(repoRoot, f)}:${n}`)
      )
    );
    for (const [key, why] of Object.entries(NO_HEALTHCHECK)) {
      assert.ok(declared.has(key), `${key} is no longer declared`);
      assert.ok(why.length > 20, `${key} needs a reason`);
    }
  });

  it('when vm-ops-api runs in the sidecar network namespace, then it probes itself with its own binary', () => {
    const file = join(repoRoot, 'infra/oracle-cloud/vm-ops-api/docker-compose.yml');
    const { services } = parse(readFileSync(file, 'utf8'), { merge: true });
    assert.deepEqual(services['ops-api'].healthcheck.test, ['CMD', 'vm-ops-api-rs', '--healthcheck']);
    assert.equal(services['ops-api'].network_mode, 'service:tailscale');
    assert.ok(services['ops-api'].depends_on.includes('tailscale'));
  });

  it('when vm-ops-api opens its two ports, then the CI port has its own seed and no port is published', () => {
    const dir = join(repoRoot, 'infra/oracle-cloud/vm-ops-api');
    const { services } = parse(readFileSync(join(dir, 'docker-compose.yml'), 'utf8'), { merge: true });
    const env = services['ops-api'].environment;
    assert.ok(env.includes('CI_PORT=${CI_PORT:-}'), 'the CI port is passed through');
    assert.ok(env.includes('VM_OPS_API_CI_AUTH_TOKEN=${VM_OPS_API_CI_AUTH_TOKEN:-}'), 'the CI seed is passed through');
    for (const name of ['ops-api', 'tailscale']) {
      assert.equal(services[name].ports, undefined, `${name} publishes a port`);
    }
    assert.match(readFileSync(join(dir, '.env.defaults'), 'utf8'), /^CI_PORT=8081$/m);
  });

  it('when a service is a tailscale sidecar, then it probes `tailscale status --json`', () => {
    for (const file of files) {
      const svc = loadServices(file).tailscale;
      if (!svc) continue;
      assert.match(
        svc.healthcheck.test.join(' '),
        /tailscale status --json/,
        relative(repoRoot, file)
      );
    }
  });

  it('when agent-shell runs its nested dockerd, then agent-dind probes `docker info` on the dind socket', () => {
    const services = loadServices(
      join(repoRoot, 'infra/oracle-cloud/agent-shell/docker-compose.prod.yml')
    );
    const test = services['agent-dind'].healthcheck.test.join(' ');
    assert.match(test, /docker -H unix:\/\/\/run\/dind\/docker\.sock info/);
  });
});
