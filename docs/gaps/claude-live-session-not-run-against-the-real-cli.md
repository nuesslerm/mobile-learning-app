---
status: open
area: remote-chat-agent
paths:
  - "backend/remote-chat-agent/cli-driver/src/domain/claude.rs"
  - "backend/remote-chat-agent/cli-driver/src/application/claude_sessions.rs"
  - "libraries/agent-api/exec-testkit/src/bin/scripted-cli.rs"
  - "backend/remote-chat-agent/rs/tests/flow/real_backends.rs"
opened: 2026-11-22
last_verified: 2026-11-22
links:
  - "docs/solutions/architecture/claude-code-stream-json-protocol.md"
---
# Claude Code's live session has run against authored conversations and, in CI only, one real turn and its follow-up

## What is broken

A Claude Code thread runs on one `claude -p --input-format stream-json` process (`ClaudeSessions`). Every test of it plays the scripted `claude` of `exec-testkit`, which speaks the protocol as `docs/solutions/architecture/claude-code-stream-json-protocol.md` reads it from the binary's schemas and from another client's recordings (Claude Code 2.1.111 to 2.1.281), not as a live 2.1.330 turn showed it. The only real evidence is the `Real backends` workflow's `turn (claude)` leg: one turn that writes a file and says PONG, and a second prompt on the same thread that must answer PONG again. It covers the handshake, `--session-id`, a user message with a `uuid`, the stream and tool frames of one turn, the `result` frame matched by `user_message_uuids`, and that the process keeps the conversation between two prompts.

Not seen by any run, in the order it matters:

- **Steer.** A `priority:"now"` message and the two `result` frames it causes (the aborted one first). The service ends the turn on the result that names the steer's uuid; a CLI that names the uuids differently would leave the turn open until its process dies.
- **Approvals and questions.** `can_use_tool` as the CLI writes it (the shapes are derived from the schema and SDK-level recordings), the allow and deny answers, `AskUserQuestion` answered through `updatedInput.answers`, and `control_cancel_request`.
- **Interrupt.** `interrupt` with `cancel_queued` followed by a live `result`; no recording has one. The service kills the process after 15 seconds if the turn does not end.
- **`set_permission_mode` and `set_model` between turns of a live process** (the answers were seen with no turn running).
- **Resume.** `--resume <id>` after the idle close or a crash, in the same `cwd` and `CLAUDE_CONFIG_DIR`, and whether the CLI reports the same `session_id` on `system/init`.
- **A long-lived process** on the A1 VM: footprint per thread, many threads at once, a process held for the idle time (`RCA_CLAUDE_IDLE_SECS`).
- **The service's tool server** passed in `--mcp-config` of a live process, its bearer kept for the life of the process.

## Why blocked

Running the real CLI needs the account's token and a model turn, and the account's quota is shared with the sessions that develop it. The real proofs run in CI only.

## What unblocks it

Record a turn with a steer, an approval, a question, an interrupt and a resume against the pinned version with `wf mimic record`, replace the authored conversations of the scripted `claude` with them, and compare each frame the service sends with what the CLI did with it. The first green `turn (claude)` leg of the `Real backends` workflow is the first evidence for the handshake, the user message, the result match and the kept conversation.
