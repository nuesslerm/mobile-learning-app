#!/usr/bin/env node
// On a dev machine: build the Rust wf with cargo, sync the generated agent config, and link the slot registry.
// A backend Dockerfile's filtered install (DOCKER_BUILD=true) and CI (install-wf builds wf) skip all of it.
import { execSync } from 'node:child_process';
import {
  existsSync,
  lstatSync,
  mkdirSync,
  readlinkSync,
  symlinkSync,
  unlinkSync,
} from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

function run(cmd) {
  execSync(cmd, { stdio: 'inherit' });
}

// Upsert a symlink from repo-root/.local/registry.json to wf's machine-local
// slot registry ($XDG_STATE_HOME/wf-registry/registry.json, default
// ~/.local/state/wf-registry/registry.json), so the registry is always
// visible in-tree without guesswork. Non-fatal: a developer convenience, not
// required in CI.
function ensureRegistrySymlink() {
  const repoRoot = join(fileURLToPath(import.meta.url), '..', '..');
  const localDir = join(repoRoot, '.local');
  const linkPath = join(localDir, 'registry.json');
  const xdg = process.env.XDG_STATE_HOME;
  const base = xdg ?? join(process.env.HOME || '~', '.local', 'state');
  const target = join(base, 'wf-registry', 'registry.json');

  try {
    mkdirSync(localDir, { recursive: true });

    try {
      const stat = lstatSync(linkPath);
      if (stat.isSymbolicLink() && readlinkSync(linkPath) === target) return; // already correct
      unlinkSync(linkPath);
    } catch {
      // doesn't exist, fine
    }

    symlinkSync(target, linkPath, 'file');
  } catch (err) {
    console.warn('[ensure-registry-symlink] skipped:', err.message);
  }
}

if (process.env.DOCKER_BUILD === 'true') {
  // Nothing: the image builds only its own service.
} else if (process.env.CI === 'true') {
  // A workflow gets wf from .github/actions/install-wf (incremental, on the runner's own volume); a cold release build
  // here took the VM's runner 15 to 40 minutes per job and starved Metro and the device.
  console.warn('[postinstall] CI: the wf binary comes from install-wf, not built here');
} else {
  // The wf CLI is a Rust binary: cargo builds it once (and again when a source changed), with the t3-serve, rca-exec and wf-mimic
  // binaries it runs beside it. A machine without a toolchain keeps whatever binaries are already there and says so.
  try {
    execSync('cargo --version', { stdio: 'ignore' });
    run('cargo build --release -p wf -p t3-serve -p rca-exec -p wf-mimic --manifest-path clis/Cargo.toml');
  } catch {
    console.warn('[postinstall] no cargo on PATH: the wf binary was not built');
  }
  // The sync lives in the Rust wf binary; a fresh clone has none until cargo builds it.
  if (existsSync(new URL('../clis/target/release/wf', import.meta.url))) {
    run('pnpm run sync:agent-config');
  }
  ensureRegistrySymlink();
}
