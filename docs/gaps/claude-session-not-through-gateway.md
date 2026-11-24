---
status: open
area: remote-chat-agent
paths:
  - "backend/remote-chat-agent/group-agent/src/runner.rs"
  - "backend/remote-chat-agent/cli-driver/src/application/claude_sessions.rs"
  - "backend/remote-chat-agent/cli-driver/src/domain/provider_env.rs"
opened: 2026-11-22
last_verified: 2026-11-22
links:
  - "docs/gaps/codex-session-not-through-gateway.md"
  - "docs/gaps/model-gateway-unproven-against-real-binaries.md"
---
# A Claude Code thread is never routed through the model gateway

## What is broken

A Claude Code turn used to be routed through the gateway when the service held an Anthropic key: the command of the turn began with `ANTHROPIC_BASE_URL`, the turn's token as `ANTHROPIC_API_KEY` and an empty `CLAUDE_CODE_OAUTH_TOKEN`. Claude Code now runs as one process for each thread that outlasts a turn (`ClaudeSessions`, started through the executor's `session_open`), so there is no command per turn: the process keeps the environment it started with, and `session_open` lets the caller add only the flags of its list (`SESSION_FLAGS`), never an environment variable. A Claude Code thread therefore runs with the login of the workspace host, whether or not the service holds an Anthropic key, and the key is never given to the host.

## Why blocked

A token that dies with each turn cannot be given to a process that lives for many. The executor has no way to set the environment of a session, and giving it one means choosing which variables a caller may set (a bearer for the gateway is one; `PATH` or `ANTHROPIC_BASE_URL` pointing elsewhere would be an escape).

## What unblocks it

Add an `env` argument to `session_open` limited to a list of names (`ANTHROPIC_BASE_URL`, `ANTHROPIC_API_KEY`, `CLAUDE_CODE_OAUTH_TOKEN`), mint the thread's route in `ClaudeSessions::start` with a token that lives as long as the process does (revoked when it ends), and prove with the real CLI that the process takes a changed base URL at its start and calls the gateway.
