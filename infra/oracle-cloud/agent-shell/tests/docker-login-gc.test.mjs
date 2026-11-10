import assert from 'node:assert';
import { spawnSync } from 'node:child_process';
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { afterEach, beforeEach, describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';

const script = join(dirname(fileURLToPath(import.meta.url)), '..', 'docker-login.sh');
const DISK_BYTES = 200_000_000_000;

let dir;
let log;

function stub(name, body) {
  const path = join(dir, name);
  writeFileSync(path, `#!/bin/bash\n${body}\n`);
  chmodSync(path, 0o755);
}

function runScript() {
  return spawnSync('bash', [script], {
    env: { PATH: `${dir}:/usr/bin:/bin`, STUB_LOG: log },
    encoding: 'utf8',
  });
}

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'docker-login-gc-'));
  log = join(dir, 'calls.log');
  stub('docker', 'echo "docker $*" >> "$STUB_LOG"; exit 0');
  stub('df', `printf 'Size\\n${DISK_BYTES}\\n'`);
});

afterEach(() => {
  rmSync(dir, { recursive: true, force: true });
});

describe('docker-login.sh garbage collection', () => {
  test('given a 200 GB disk, when the script runs, then it prunes unused images', () => {
    const result = runScript();

    assert.strictEqual(result.status, 0, result.stderr);
    assert.match(readFileSync(log, 'utf8'), /^docker image prune -af$/m);
  });

  test('given a 200 GB disk, when the script runs, then build-cache limits are fractions of that disk', () => {
    runScript();

    const maxUsed = (DISK_BYTES * 40) / 100;
    const minFree = (DISK_BYTES * 25) / 100;
    assert.match(
      readFileSync(log, 'utf8'),
      new RegExp(`^docker buildx prune -f --max-used-space ${maxUsed} --min-free-space ${minFree}$`, 'm'),
    );
  });

  test('given a smaller disk, when the script runs, then the limits shrink with it', () => {
    stub('df', "printf 'Size\\n100000000000\\n'");

    runScript();

    assert.match(
      readFileSync(log, 'utf8'),
      /^docker buildx prune -f --max-used-space 40000000000 --min-free-space 25000000000$/m,
    );
  });

  test('given the builder is not running, when the script runs, then it bootstraps it before pruning', () => {
    runScript();

    const calls = readFileSync(log, 'utf8').split('\n');
    const bootstrap = calls.indexOf('docker buildx inspect --bootstrap');
    const prune = calls.findIndex((c) => c.startsWith('docker buildx prune'));
    assert.ok(bootstrap >= 0 && prune > bootstrap, calls.join('\n'));
  });
});
