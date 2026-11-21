---
status: open
area: rca
paths:
  - "backend/remote-chat-agent/group-agent/src/host.rs"
  - "backend/remote-chat-agent/rs/src/infrastructure/mcp_address.rs"
  - "backend/remote-chat-agent/rs/src/api/routes/exec_remote.rs"
  - "backend/remote-chat-agent/service-core/src/application/agent_groups/mod.rs"
opened: 2026-11-18
last_verified: 2026-11-18
links: []
---
# The agent CLIs now run at the workspace host; follow-ups remain

A turn runs its CLI at the workspace host through the executor's `run_command` (owner direction, 18 Nov, `docs/plans/rca-client/architecture.md`). Not done yet:

- **The service's tool server needs `AGENT_PUBLIC_URL` for a remote host.** `/mcp/<thread>` answers a loopback caller or the bearer token of the thread's running turn (which the CLI is told in its tool-server config), and `LoopbackMcp` names `AGENT_PUBLIC_URL` when set; with it unset a remote host is told the loopback address and cannot reach it.
- **Provider credentials still live at the service.** The accounts group (`provider.auth.*`, usage), `provider_env` and the providers' root assume the service holds the logins; the host's own logins are what a turn now uses. Moving them is plan item 3 of `docs/plans/rca-parity/plan.md`.
- **No real CLI has run through it.** The tests use scripted CLIs; R16 of #83 is the check with the real ones, one hosted job per backend.
- **The CLI-presence refusal is partial.** `turn.start` and `schedule.runNow` refuse a turn naming a missing CLI with `CLI_NOT_INSTALLED` (a `--version` probe of up to 5 s, `VERSION_TIMEOUT_MS`, before the turn starts); a queued, retried or restarted turn that names a missing CLI still ends as `CLI_START_FAILED`. `agents.detect` lists a driver only when the executor's `cli_detect` reports its CLI present (a CLI that is there but did not answer stays listed with `error`); `tools` still lists git, gh and glab with `installed`. A client that wants to offer an install of an absent CLI no longer gets its name from the listing. `provider.list` (accounts group, login UI) deliberately still lists every driver, installed or not.
- **The `CLI_NOT_INSTALLED` and missing-CLI fixtures are authored doubles.** No real `claude`, `codex` or `opencode` was absent from a recorded host: exchange `d7` of `libraries/agent-api/exec-proc/tests/fixtures/cli_detect.json` (`codex` not installed, asked alone as the service asks one program per turn) and the `turn.start.json` exchange that replays the refusal (`codex is not installed on the executor of workspace w1`, actions `cli_detect` only) are written by hand, like every fixture of `libraries/agent-api` (`fixture-provenance.json`, kind `authored`). Exchange `d6` (version `2.1.7 (Claude Code)`, `signed_in: false`) is authored too. A recording of a host without the CLI waits on a real host that lacks one (R16).
