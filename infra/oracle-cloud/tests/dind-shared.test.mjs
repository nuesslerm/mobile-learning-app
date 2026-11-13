import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { parse } from 'yaml';
import { loadCompose } from './compose-load.mjs';

const dir = join(dirname(fileURLToPath(import.meta.url)), '..');
const raw = (app) => parse(readFileSync(join(dir, app, 'docker-compose.prod.yml'), 'utf8'), { merge: true });
const base = parse(readFileSync(join(dir, 'dind.yml'), 'utf8')).services.dind;
const DAEMONS = [['ci-runner', 'builder'], ['ci-runner', 'droid'], ['agent-shell', 'agent-droid']];

describe('given the docker-in-docker daemons of ci-runner and agent-shell', () => {
  it('when a daemon is declared, then it extends the one definition in dind.yml and repeats none of it', () => {
    for (const [app, name] of DAEMONS) {
      const service = raw(app).services[name];
      assert.deepEqual(service.extends, { file: '../dind.yml', service: 'dind' }, `${app}/${name}`);
      for (const key of ['image', 'privileged', 'network_mode', 'command', 'healthcheck', 'depends_on', 'restart', 'logging', 'environment']) assert.equal(service[key], undefined, `${app}/${name} redefines ${key}`);
    }
  });

  it('when the definition is read, then it is a pinned privileged daemon on one unix socket, no port, in the sidecar namespace, with no volumes', () => {
    assert.match(base.image, /^docker:\d+\.\d+\.\d+-dind$/);
    assert.equal(base.privileged, true);
    assert.equal(base.network_mode, 'service:tailscale');
    assert.deepEqual(base.command, ['dockerd', '--host=unix:///run/dind/docker.sock']);
    assert.ok(base.healthcheck, 'the definition carries the healthcheck');
    assert.equal(base.volumes, undefined);
    assert.equal(base.ports, undefined);
    assert.equal(base.environment, undefined, 'no TLS directory: that would add a tcp listener');
  });

  it('when a daemon is read in full, then it holds its own socket and its own data volume, named for it', () => {
    for (const [app, name] of DAEMONS) {
      const service = loadCompose(join(dir, app, 'docker-compose.prod.yml')).services[name];
      const prefix = name.replace(/^agent-/, '');
      assert.deepEqual(service.volumes, [`${prefix}-socket:/run/dind`, `${prefix}-data:/var/lib/docker`], `${app}/${name}`);
      assert.ok(service.healthcheck && service.depends_on.includes('tailscale'), `${app}/${name}`);
    }
  });

  it('when the Android verbs run, then the runner and the agent-shell tools reach the device through DEVICE_DOCKER_HOST on the droid socket, builds untouched', () => {
    const runner = raw('ci-runner').services['gh-runner'];
    const app = raw('agent-shell').services.app;
    for (const service of [runner, app]) {
      assert.equal(service.environment.DEVICE_DOCKER_HOST, 'unix:///run/droid/docker.sock');
      assert.ok(service.volumes.includes('droid-socket:/run/droid'));
    }
    assert.equal(runner.environment.DOCKER_HOST, 'unix:///run/builder/docker.sock');
    assert.ok(runner.volumes.includes('builder-socket:/run/builder'));
    assert.equal(app.environment.DOCKER_HOST, 'unix:///run/dind/docker.sock');
    assert.ok(runner.depends_on.includes('droid'));
  });
});
