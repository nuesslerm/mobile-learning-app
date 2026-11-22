---
status: open
area: remote-chat-agent
paths:
  - "backend/remote-chat-agent/cli-driver/src/domain/codex.rs"
  - "backend/remote-chat-agent/cli-driver/src/application/codex_sessions.rs"
  - "backend/remote-chat-agent/group-agent/src/host.rs"
  - "clis/rca-exec/src/adapters/sessions.rs"
opened: 2026-11-22
last_verified: 2026-11-22
links:
  - "docs/solutions/architecture/codex-app-server-protocol.md"
---
# Codex's live session has run only against recordings and authored conversations, never against the real CLI

## What is broken

A Codex thread runs on `codex app-server`, a process the service keeps per workspace and talks to over its stdio (`CodexSessions`, through the executor's `session_open` and `session_send`). Every test of it plays either a recording of Codex's own app-server that another client wrote (`libraries/agent-api/exec-testkit/transcripts/codex-app-server`, Codex 0.156.1 and 0.157.0) or a conversation a test authored, so a difference between those and 0.171.0 passes every test.

What the tests have not seen, in the order it matters:

- **A request of this client, answered by the real server.** The recordings are another client's: it sends `capabilities.experimentalApi: true` and `optOutNotificationMethods`, and `cwd`, `config` on `thread/start`. This client sends neither capabilities nor `cwd`/`config` (so no experimental field is in play), `thread/start {model?}`, `thread/resume {threadId}` and `turn/start {threadId, input, approvalPolicy, approvalsReviewer, sandboxPolicy, summary, model?}`. The server may refuse one of those; no recording shows it.
- **`thread/resume`.** Its answer's shape (the id is read from `thread.id`), whether it re-emits `thread/started`, and the text of its errors (a thread with no rollout, an archived one: the service retries once after `thread/unarchive` when the text names an archive, which is T3's rule, not a recording) are not recorded.
- **Items and requests no recording holds.** `fileChange` items (translated at completion from `changes`), `fileChange` and `permissions` approvals, `mcpToolCall` and the other tool items, `item/reasoning/summaryTextDelta`, the `error` notification with `willRetry`, and a `failed` turn are read from the 0.171.0 schema, not from a run.
- **What the executor gives the process.** It starts with the executor's home and `PATH` (the way the CLI logins do), in the workspace's directory, with no `CODEX_HOME` of its own. A Codex login that lives somewhere else is not found.
- **Steer and interrupt corner cases.** `turn/steer` with a stale `expectedTurnId`, on a finished turn, or while an approval waits; `turn/interrupt` while a command runs (a running command survives it, per the protocol note) are not recorded. The service ends such a turn only when the server says so, or kills the process after 15 seconds.
- **Many threads at once in one process** under load (the recordings are one thread each).
- **`ask_user` for Codex.** The service's tool server is not given to Codex, so a Codex turn cannot ask the person a question; only approvals reach the phone.

## Why blocked

Running the real CLI needs a Codex login and a model turn; none exists in the sandbox, and a run from a development session would use a person's own sign-in. The owner has a second Codex login in its own `CODEX_HOME` and vault secret for the real-CLI tests.

## What unblocks it

The pipeline's `codex-session-proof` job (`docs/solutions/testing/real-codex-session.md`) runs `CodexSessions` over the service's own process adapter against the pinned real `codex app-server` on the second Codex login: two turns of one thread with a close and a `thread/resume` between them, asserting the neutral events (session id, text, done). Its first green run is the first evidence for `thread/start`, `thread/resume` (the answer's shape and that a resumed thread keeps the conversation), `turn/start` and the process environment; it does not cover steer, interrupt, approvals or many threads. The real-backends job's `turn (codex)` leg (`backend/remote-chat-agent/rs/tests/flow/real_backends.rs`, run with the pinned CLI and the credential) now runs a Codex turn through this path: a green run is the first evidence for the first bullet and for the executor's start of the process. Record a turn with a steer, an approval, an interrupt and a resume against the pinned version with `wf mimic record`, replace the authored runs of `agent_codex.rs` with the recordings, and compare each request of the service with the server's answer.
