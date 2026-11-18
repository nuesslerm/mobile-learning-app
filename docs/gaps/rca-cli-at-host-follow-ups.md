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
# The agent CLIs now run at the workspace host; four follow-ups remain

A turn runs its CLI at the workspace host through the executor's `run_command` (owner direction, 18 Nov, `docs/plans/rca-client/architecture.md`). Not done yet:

- **The service's tool server needs `AGENT_PUBLIC_URL` for a remote host.** `/mcp/<thread>` answers a loopback caller or the bearer token of the thread's running turn (which the CLI is told in its tool-server config), and `LoopbackMcp` names `AGENT_PUBLIC_URL` when set; with it unset a remote host is told the loopback address and cannot reach it.
- **Provider credentials still live at the service.** The accounts group (`provider.auth.*`, usage), `provider_env` and the providers' root assume the service holds the logins; the host's own logins are what a turn now uses. Moving them is plan item 3 of `docs/plans/rca-parity/plan.md`.
- **No real CLI has run through it.** The tests use scripted CLIs; R16 of #83 is the check with the real ones, one hosted job per backend.
