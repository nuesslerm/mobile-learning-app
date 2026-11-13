---
status: open
area: agent-shell
paths:
  - "infra/oracle-cloud/agent-shell/Dockerfile"
  - ".github/actions/install-wf/**"
  - ".github/workflows/clis.yml"
  - "clis/wf-vmops/src/vm_exec.rs"
opened: 2026-10-04
last_verified: 2026-10-20
links:
  - archive/solutions/runtime-errors/runbook-on-vm-rendered-script-bare-wf-resolves-to-the-image-baked-binary-and-the.md
---
# agent-shell's global `wf` runs the dev trunk's build, not the checkout's

## What is broken

When this gap was opened, `/usr/local/bin/wf` exec'd `/usr/local/bin/wf-bundle.cjs`, copied in at image
build. A new subcommand built in the checkout was invisible: the baked `wf` answered the then-current
gemini `regen` subcommand with `error: unknown command 'regen'`, while the checkout's own build of the
CLI (then the TypeScript `wf.js`, since deleted, run with `node`) with the same arguments
worked.

The bundle is gone (the JS bundle and every TypeScript command were removed from `wf`; it is now the Rust
CLI under `clis/wf/`). `/usr/local/bin/wf` is now a symlink to `/opt/clis/current/wf`
(`infra/oracle-cloud/agent-shell/Dockerfile:174`), the VM's one build on the `clis` volume, which
`docker-compose.prod.yml` mounts read-only. `.github/workflows/clis.yml` (the `clis` job of `pipeline.yml`)
writes that build through `.github/actions/install-wf` for every push to `dev` whose `plan` job succeeded
and whose `flow-tests` and `typecheck-lint-unit-tests` jobs did not fail; a push with red tests leaves
`/opt/clis/current` on the previous build. What remains is narrower: the global `wf` is the `dev` trunk's
build, so a subcommand built only in the active checkout is still invisible to it, and `wf android lease`
on the VM can run an even older build (see "Re-checked 2026-10-19").

## Why blocked

The original fix (change the image to symlink `/usr/local/bin/wf` into the active checkout's build and
redeploy agent-shell) was out of scope for the task that found it. The image now symlinks it to the
shared `clis` build instead, so pointing it at a per-checkout target directory would contradict the
one-build design rather than be a small image tweak.

## What unblocks it

Land the subcommand on `dev` with green tests, so `clis.yml` publishes it to `/opt/clis/current`, or give agent-shell a
wrapper that prefers the active checkout's binary when one exists. Until then call
`<repo>/clis/target/release/wf <args>` inside agent-shell after building
(`cargo build --release --manifest-path <repo>/clis/Cargo.toml -p wf`).

## Re-checked 2026-10-19

The baked bundle is gone: `infra/oracle-cloud/agent-shell/Dockerfile:174` links `/usr/local/bin/wf` to
`/opt/clis/current/wf`, and only the `clis` job moves `current` (`.github/workflows/clis.yml:27-29`,
`.github/actions/install-wf/action.yml:55`, gated in `.github/workflows/pipeline.yml`). The one check is
that job's step at `clis.yml:33`, which asserts `current/wf build-sha` equals the pushed `clis/` tree; no test
checks which build agent-shell's `wf` resolves to, and `infra/oracle-cloud/agent-shell/e2e-wf-smoke.sh:22-37`,
run by hand, only checks `wf --help`. Still failing: the login PATH (`infra/oracle-cloud/agent-shell/entrypoint.sh:182`)
has no checkout bin and no `~/.local/bin`, so a bare `wf` is the trunk build and a subcommand built only in the
checkout still answers `error: unknown command '<name>'` (`clis/wf-kit/src/cli.rs:95`). The `step_wf` link in
`.agents/setup.sh:68-75` does not help: a process with agent-shell's container env (`docker exec`, `wf vm exec`)
inherits `ENV DOCKER_BUILD=true` (`infra/docker/agent-base/Dockerfile:145`, via `FROM` at
`infra/oracle-cloud/agent-shell/Dockerfile:42`), which skips it (`setup.sh:147`); an SSH login does not get that
variable (`entrypoint.sh:155-189` writes only named ones to `/etc/environment`), so there it runs, but it needs a
`cargo` neither image installs and links `~/.local/bin/wf`, which that PATH lacks; and it never takes the link from
another live checkout (`setup.sh:73`). A second
route is worse: `clis/wf-vmops/src/vm_exec.rs:324` runs `/workspace/home/wf-bin/wf` when it exists and only then falls
back to `wf`, but the only writer of that path, `wf ship worker` (the former `wf test sync-wf`, which installed
wf for the test-queue worker), was deleted in 92da772d8. So
`wf android lease` without `--local` (`clis/harness-android/src/android_lease.rs:342`, `:360`) runs either a frozen
pre-92da772d8 binary left on the `/workspace` volume, which outranks even the trunk build, or the trunk build,
and never the caller's branch. The workaround above is unchanged.

## Re-checked 2026-10-20

I checked the files the gap names and the history since the 19 Oct re-check. None of them changed: Dockerfile:174, entrypoint.sh:182, setup.sh:68-75/:147, vm_exec.rs:480 and android_lease.rs:342/:360 read as the 19 Oct note describes. agent-shell's bare `wf` is still the dev trunk's /opt/clis/current build. A non-local `wf android lease` still prefers the orphaned /workspace/home/wf-bin/wf, and no code writes that path any more. The owner rule from 3002218c6 (never `wf android lease` by hand) makes that second route matter less in practice, but the code is unchanged.

Out of date above:

- `clis/wf/src/main.rs:161` (unknown command): the eprintln is now at clis/wf-kit/src/cli.rs:95.
- The workaround should note that, since 3002218c6 (20 Oct), the owner rule forbids running `wf android lease` by hand. The vm_exec.rs:480 route is now reached only by tooling, not by an agent following the docs.

Update 2026-11-13: the VM runner and its `clis-sync` container are gone and CI runs hosted, so no job writes the `clis` volume any more: `/opt/clis/current` stays on the cloud-init `bootstrap` build until another writer exists. The `clis` job still publishes the `wf-latest` release; agent-shell's global `wf` follows it only if something installs it.
