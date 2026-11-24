---
status: open
area: remote-chat-agent
paths:
  - "backend/remote-chat-agent/group-agent/src/runner.rs"
  - "backend/remote-chat-agent/cli-driver/src/domain/provider_env.rs"
opened: 2026-11-22
last_verified: 2026-11-22
links:
  - "docs/gaps/model-gateway-unproven-against-real-binaries.md"
---
# A Codex thread is never routed through the model gateway

## What is broken

A turn of opencode is routed through the gateway when the service holds a key for the provider: the CLI is given the gateway's address and a token of that turn alone. Codex used to be too (`model_providers.gateway` and `CODEX_GATEWAY_TOKEN` on the command line of each `codex exec`). Codex now runs as one server for every thread of a workspace, so there is no command line per turn and an environment variable is the server's for good; a token that dies with each turn cannot be given to it that way. A Codex turn therefore runs with the login of the workspace host, whether or not the service holds an OpenAI key, and the key is never given to the host either.

## Why blocked

The per-thread way to name a provider is `modelProvider` and `config` on `thread/start` and `thread/resume` (`model_providers.<id>.experimental_bearer_token` is in the 0.171.0 binary). Whether a thread that is already loaded takes a changed provider on its next turn, which a token minted for each turn needs, is in no recording.

## What unblocks it

Run one thread against the real server with a provider named in `thread/resume` for each turn, and see whether the second turn calls the gateway with the new token. If it does, mint the route in `CodexSessions::thread_of` and put it in the request; if not, mint a token that lives as long as the thread is loaded.
