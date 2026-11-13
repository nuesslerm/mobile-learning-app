---
status: open
area: offload
paths:
  - "clis/offload/src/dispatch_run.rs"
  - "clis/offload/src/runbook.rs"
  - "backend/remote-chat-agent/service-core/src/application/runs.rs"
opened: 2026-10-27
last_verified: 2026-10-30
links: []
---
# `wf offload ask --runbook` still drives a local opencode, and a remote run reports no token usage

## What is broken

Every other `wf offload` mode runs on the rca service (`POST /runs`, `GET /runs/log`, `POST /runs/cooldowns/clear`). Two things are left:

- `--runbook` runs `dispatch_local` (the old in-process ladder and `opencode run` child), because the runbook agent drives a browser and a device from a real checkout, and the service has only a staging directory of the files it was sent. Its `preflight`, `--live` tee and local ledger code live on only for this path.
- A remote run's answer now carries the token usage (`usage`), but the service's run-log row still has no tokens or cost, so `--stats` of the service's log shows them as zero for remote rows.
- A run that needs the repository (`/cheap` on code the caller did not name with `--files`, the cf researcher and reviewer agents, runbooks) sees only the files it is sent. A checkout the service runs in, as below, would fix all of these.
- `--fork` of a session is not supported.
- `opencode run` sees the directory it runs in; a remote run sees only the files it is sent (`-f`, `--files`), so a prompt that asks the model to search the checkout finds nothing unless the files travel. The parity e2e passes `--files` for those prompts. A tool trace on stderr is rebuilt from the events for read, write, edit, bash, glob and grep; other tools print one generic line. Unknown-option exit codes follow `wf` (2), opencode's are 1.

## What would fix it

- A checkout the service can run a runbook in (a git worktree of a pushed ref, with `wf` and the device harness on `PATH`), then delete `dispatch_local`, `preflight`, `live`, the file ledger and `--start-model`'s local rotation.
- `runs.rs` returning the usage opencode reports per attempt, and writing it to the run-log row the way the local path did.
- The agent-shell `vm-ledger` routes are unused by `wf` now and can go with the next vm-ops-api change (`/cooldowns` is gone).

## Where a remote run still differs from `opencode run` (found by the parity e2e, `clis/offload/tests/e2e/parity*.rs`)

- A plugin or tool that writes with a relative path writes relative to the opencode server's working directory, not the project's: use the `directory` the plugin gets. Process environment (`{env:NAME}` in a config, `process.env`) is the service's, not the caller's.
- A local MCP server is sent with the project only when it runs a script of the project with `node`, `bun`, `python3` or `sh`; one that installs a package first (`npx`) is dropped from the config the service gets. LSP servers and `OPENCODE_*` environment settings of the caller are not carried.
- The service has no checkout: a file a tool reads or changes is there only if it was sent (`--files`, `-f`); the project's config, instructions, agents, commands, skills (`.opencode`, `.claude/skills`, `.agents/skills`), plugins and tools, and what `{file:...}`, `@path` and `instructions` name, are sent by `ask` itself.
- Background jobs that outlive a turn keep running on the service in the session's directory; what they make comes back with the next turn on that session (the baseline of a session is what was last delivered), not with the turn that started them.
- `subagent_depth` and `experimental.policies` of the opencode docs are OpenCode 2 settings; on 1.19 a nested subagent is not started with depth 2 either, on both sides.
- The unknown-option exit code of `ask` is `wf`'s (2), opencode's is 1; `--fork`, `--share`, `--thinking`, `--print-logs`, `--pure` and the server and account commands of opencode are not carried (opencode-commands.json says why for each).
