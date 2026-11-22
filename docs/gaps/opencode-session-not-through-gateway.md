---
status: open
area: remote-chat-agent
paths:
  - "backend/remote-chat-agent/group-agent/src/runner.rs"
  - "backend/remote-chat-agent/group-agent/src/gateway.rs"
  - "backend/remote-chat-agent/cli-driver/src/application/opencode_sessions.rs"
opened: 2026-11-22
last_verified: 2026-11-22
links:
  - "docs/gaps/codex-session-not-through-gateway.md"
  - "docs/gaps/model-gateway-unproven-against-real-binaries.md"
---
# An opencode thread is never routed through the model gateway

## What is broken

A turn of opencode was routed through the gateway when the service held a key for the provider: the command of the turn carried `OPENCODE_CONFIG_CONTENT` with the gateway's `baseURL` and a token that lived for that turn alone. opencode now runs as one `opencode serve` for every thread of a workspace (`OpencodeSessions`), so there is no command line per turn, and the environment and config of a server are its own for as long as it lives. The per-turn route (`Runner::host_command`, `through_gateway`, the route table of a running turn) is deleted. An opencode thread runs with the login and the provider keys of the workspace host, whether or not the service holds a key for the provider, and the key is never given to the host. Nothing calls `TurnGateway::route` any more; the port stays for the route a server could be given.

## Why blocked

The token minted for a turn cannot be put in a server that outlives the turn. The routes opencode offers to change a provider after the server started are `PATCH /config` (it wrote `config.json` into the working directory when it was tried, `docs/solutions/architecture/opencode-server-protocol.md`), `PUT /auth/{providerID}` (a key, per server and not per thread) and the provider block of `OPENCODE_CONFIG_CONTENT` at spawn. A prompt names a model and an agent but no provider options.

## What unblocks it

Mint the route for the workspace when its server starts, not for a turn: a token that lives as long as the server, in the provider block of `OPENCODE_CONFIG_CONTENT`, which `session_open` would take as a checked pseudo-flag (the executor makes the credential of the server itself, and would put the config in its environment the same way). That keeps the token out of argv and out of every event. Revoking it when the server closes is the idle close's job. Until then the gateway serves only a caller that is not an agent turn.
