---
status: blocked
area: remote-chat-agent
paths:
  - "backend/remote-chat-agent/service-core/src/domain/clis.rs"
  - "clis/wf-login/src/vnc/relay.rs"
  - "clis/wf-login/src/vnc/descriptor.rs"
opened: 2026-08-20
last_verified: 2026-10-04
links: []
---
# Claude and Codex backends of remote-chat-agent were removed unproven; the credential relay awaits a human sign-in

## What is broken

Built and deployed as the Node service, but only proven pre-login; the claude and codex gateways were then removed with it and are not ported to the Rust rewrite:
- codex driver: all known code defects fixed; a live turn ended `codex session error: Reconnecting... 2/5`. Removed with the Node service.
- claude backend: image carried `claude-agent-sdk-linux-arm64`, resume path existed; removed with the Node service.
  `CLAUDE_CODE_OAUTH_TOKEN` is registered with an empty value.
- Reactive login (`createClaudeAuthRecovery`/`createCodexAuthRecovery`) never triggered by a real auth failure. Removed with the Node service.
- The vnc-auth credential relay to agent-shell (`clis/wf-login/src/vnc/relay.rs`, posting to vm-ops-api's `POST /agent-shell/credentials`) never delivered a real login.

## Why blocked

Each needs a person to complete an OAuth sign-in (`claude setup-token`, `wf secret login codex`);
no agent may supply that credential, and opencode must never hold an Anthropic credential.

## What unblocks it

A human runs `wf secret login claude` and `wf secret login codex` and signs in. Then check the relay
delivery and the parity runbook's leg 4; once a build ports the removed gateways, re-run the two `POST /api/chat` smoke
tests with `cli: "claude"`/`"codex"`.
