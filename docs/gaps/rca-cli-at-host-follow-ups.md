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
# The agent CLIs now run at the workspace host; five follow-ups remain

A turn runs its CLI at the workspace host through the executor's `run_command` (owner direction, 18 Nov, `docs/plans/rca-client/architecture.md`). Not done yet:

- **The service's tool server needs `AGENT_PUBLIC_URL` for a remote host.** `/mcp/<thread>` answers a loopback caller or the bearer token of the thread's running turn (which the CLI is told in its tool-server config), and `LoopbackMcp` names `AGENT_PUBLIC_URL` when set; with it unset a remote host is told the loopback address and cannot reach it.
- **Provider credentials still live at the service.** The accounts group (`provider.auth.*`, usage), `provider_env` and the providers' root assume the service holds the logins; the host's own logins are what a turn now uses. Moving them is plan item 3 of `docs/plans/rca-parity/plan.md`.
- **No real CLI has run through it.** The tests use scripted CLIs; R16 of #83 is the check with the real ones, one hosted job per backend.
- **The CLI-presence refusal is partial and its fixture is an authored double.** `turn.start` and `schedule.runNow` refuse a turn naming a missing CLI with `CLI_NOT_INSTALLED` (a `--version` probe of up to 5 s, `VERSION_TIMEOUT_MS`, before the turn starts); a queued, retried or restarted turn that names a missing CLI still ends as `CLI_START_FAILED`. `agents.detect` still lists a driver whose CLI is absent with `installed: false` rather than leaving it out (owner decision open). Exchange `d6` of `libraries/agent-api/exec-proc/tests/fixtures/cli_detect.json` (version `2.1.7 (Claude Code)`, `signed_in: false`) is written by hand, not recorded from a real `claude`, and no fixture of `turn.start.json` replays the `CLI_NOT_INSTALLED` message (the contract check asserts the substring only).

