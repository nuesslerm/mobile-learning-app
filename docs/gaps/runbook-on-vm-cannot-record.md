---
status: open
area: runbooks
paths:
  - "clis/offload/src/runbook.rs"
  - "clis/offload/src/dispatch_run.rs"
  - "docs/archive/README.md"
opened: 2026-09-06
last_verified: 2026-10-20
links: []
---
# A VM-side `wf offload ask --runbook coding-agent-live-on-vm` cannot produce a result

## What is broken

When recorded, every attempt ended `rejected` with no `steps.json`:
- opencode driver: `opencode run` exits 1 after 5-11 minutes with no error line in
  `~/.local/share/opencode/log/opencode.log`. When this was recorded the attempt was classified `reason: other`
  and nothing rotated. The Rust `dispatch_runbook` (`clis/offload/src/dispatch_run.rs`) now maps an exit-1 `other`
  to `bad-output`, which is not fatal, so a VM run rotates through the four opencode rungs of the `runbook`
  ladder (deepseek-v4.1-flash, MiniMax-M3, kimi-for-coding, k3); whether each rung exits 1 the same way has not
  been re-verified on the VM. Under the Rust driver, an opencode lane that fails on every rung exits 1 with
  `wf: opencode dispatch failed after ...` instead of a `rejected` verdict (`clis/offload/src/runbook.rs`); only the
  claude lane's `Not logged in` reply still ends `rejected`.
- claude driver (`--runbook-cli claude`): `Not logged in · Please run /login` (the VM worktree's Claude Code has no credential).
The sandbox twin `coding-agent-live-opencode.md` passed for $0.05, so the runbook itself is sound.

Both runbooks have since moved to `docs/archive/README.md`, and `wf offload ask --runbook` reads only `docs/runbooks/`
(`clis/offload/src/runbook.rs`), so the title command now fails with `ENOENT: no such file or directory` before any
dispatch. Reproducing the gap needs the runbook back under `docs/runbooks/`.

## Why blocked

The exit-1 cause is inside the opencode process on the VM; its session store is not at the default
path in agent-shell, and the error `dispatch_runbook` returns carries only the rung trail
(`opencode dispatch failed after ...: All N models were exhausted: ...`), or, for a fatal failure, the last 500
chars of that attempt's stdout and stderr, so the line that would explain it never reaches the caller. The runbook
dispatch (`caller: "runbook"`, `live: false` in `dispatch_runbook`) does not log to vm-ops-api's opencode-log store
(`GET /opencode-log/api/runs/<runId>` -> `run not found`).

## What unblocks it

Any one of: surface per-attempt raw opencode stdout/stderr in the error `dispatch_runbook`
(`clis/offload/src/dispatch_run.rs`) returns; wire the runbook dispatch into the opencode-log store; let the
VM-side runbook dispatch fall through to the `runbook` ladder's last rung, `claude-sonnet-5`, which
`dispatch_runbook` leaves out because its pool is claude-max (it already rotates the four opencode rungs on any
non-fatal failure); or a human `wf secret login claude` on the VM for the claude lane. Each one first needs the
runbook back under `docs/runbooks/`, or `wf offload ask --runbook` taught to read `docs/archive/README.md`.

## Re-checked 2026-10-20

I ran the gap's title command at 982839ee9. `wf offload ask --runbook coding-agent-live-on-vm` still fails before any dispatch with ENOENT, because runbook.rs:149 reads only docs/runbooks/ and the runbook lives in docs/archive/README.md. None of the four unblockers is in the code. dispatch_runbook still drops the claude-max rung (dispatch_run.rs:797) and still runs with `live: false` (:814), so it does not reach the opencode-log store. Its error still carries only the rung trail plus a 500-char tail (:821-823). No published run of this runbook exists (`wf test results --query "test=coding-agent-live-on-vm"` returns no runs).
