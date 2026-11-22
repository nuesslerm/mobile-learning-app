---
status: open
area: remote-chat-agent
paths:
  - "backend/remote-chat-agent/group-gateway/src/lib.rs"
  - "backend/remote-chat-agent/rs/src/infrastructure/gateway_upstream.rs"
  - "backend/remote-chat-agent/cli-driver/src/domain/provider_env.rs"
opened: 2026-11-20
last_verified: 2026-11-20
links: []
---
# The model gateway has only run against an authored scripted upstream, and Headroom has no sidecar

## What exists

`group-gateway` (credential injection, a same-protocol reroute pinned at a turn's start, an optional compression stage), the `/gateway/{provider}` route, the per-turn token (digests in memory,
wiped by a restart) and the CLI wiring (Claude Code `ANTHROPIC_BASE_URL`, Codex `model_providers` (until Codex became a live server: `codex-session-not-through-gateway`), opencode `provider.options.baseURL`) are covered by flow tests. Every provider answer in them
is an **authored double** (`rca_group_gateway::testing::ScriptedUpstream`), not a recording of a provider; the compression sidecar is `FakeSidecar`, also authored. The CLIs are the scripted CLI
of `exec-testkit`, which logs the environment it was started with.

## What has not been seen

- A real `claude`, `codex` or `opencode` started with these settings: whether Claude Code in `-p` mode takes `ANTHROPIC_API_KEY` set to the gateway token without asking to approve it, whether it
  sends the token as `x-api-key`, whether a blank `CLAUDE_CODE_OAUTH_TOKEN` is read as unset, whether Codex accepts the inline `model_providers.gateway` table given as `-c`, and whether opencode
  takes the provider block merged into `OPENCODE_CONFIG_CONTENT`. Blocked on one dedicated low-limit API key per provider in the vault (`GATEWAY_*_API_KEY` in `registry.json`), which agents cannot set.
- A real provider's streaming answer through `HttpUpstream` (the reqwest adapter): header handling, a stream that breaks, large bodies (the route takes the service's 30 MB body limit).
- The upstream addresses of the opencode providers are the ones the repository already names (`opencode-go`, `kimi`) and OpenRouter's public base; MiniMax and DeepSeek have no entry because no
  address is recorded here.
- The egress lockdown: `EgressPolicy` is data with a test, enforced nowhere. It waits for the Claude real-binary proof.
- Headroom: the stage exists behind the `Compressor` port and is off (`GatewayParts::headroom` is `None` in `main`); no sidecar adapter exists, and no measured run over five recorded sessions
  has shown the 15 % net saving after the prompt-cache loss the plan requires before it ships. Nothing is logged in the token-diet decision log as a saving.
- A subscription login is never routed by construction (a turn is routed only when the gateway holds a key for its provider), but the provider-terms position itself is the owner's to confirm.
