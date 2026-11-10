import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterAll, describe, expect, it } from 'vitest';

const SCRIPT = fileURLToPath(new URL('./block-raw-playwright.mjs', import.meta.url));
const dir = mkdtempSync(join(tmpdir(), 'raw-pw-guard-'));
const PKG = ['play', 'wright'].join('');
writeFileSync(join(dir, 'shot.cjs'), `const { chromium } = require('/repo/node_modules/.pnpm/${PKG}@1.62.1/node_modules/${PKG}');\n`);
writeFileSync(join(dir, 'clean.cjs'), "console.log('no browser here');\n");
afterAll(() => rmSync(dir, { recursive: true, force: true }));

function fires(guardCommand, env = {}) {
  try {
    execFileSync(SCRIPT, [], { env: { ...process.env, ...env, GUARD_COMMAND: guardCommand } });
    return true;
  } catch (err) {
    if (typeof err.status === 'number') return false;
    throw err;
  }
}

describe('given a node script that requires the raw browser package from the pnpm store', () => {
  it('when the script path is literal, then it fires', () => {
    expect(fires(`node ${join(dir, 'shot.cjs')} out.png`)).toBe(true);
  });

  it('when the path goes through a variable assigned earlier in the same command, then it fires', () => {
    expect(fires(`S=${dir}; CHROME_BIN=/opt/pw-browsers/chromium timeout 150 node $S/shot.cjs out.png`)).toBe(true);
  });

  it('when the path uses ${VAR} braces from a quoted assignment, then it fires', () => {
    expect(fires(`S="${dir}" && node \${S}/shot.cjs`)).toBe(true);
  });

  it('when the variable comes from the environment, then it fires', () => {
    expect(fires('node $SCRATCH/shot.cjs', { SCRATCH: dir })).toBe(true);
  });

  it('when the variable cannot be resolved at all, then it fails closed and fires', () => {
    expect(fires('node $UNSET_SCRATCH_DIR_FOR_TEST/any.cjs')).toBe(true);
  });
});

describe('given a node script that never launches a browser', () => {
  it('when it is reached through a variable, then it does not fire', () => {
    expect(fires(`S=${dir}; node $S/clean.cjs`)).toBe(false);
  });
});
