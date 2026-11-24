---
status: open
area: remote-chat-agent
paths:
  - "backend/remote-chat-agent/cli-driver/src/domain/opencode.rs"
  - "backend/remote-chat-agent/cli-driver/src/application/opencode_sessions.rs"
  - "clis/rca-exec/src/adapters/opencode_bridge.rs"
  - "libraries/agent-api/exec-testkit/src/opencode_live.rs"
opened: 2026-11-22
last_verified: 2026-11-22
links:
  - "docs/solutions/architecture/opencode-server-protocol.md"
  - "docs/gaps/codex-live-session-not-run-against-the-real-cli.md"
---
# opencode's live session has not run a model turn against the real server in any session of ours

## What is broken

An opencode thread runs in `opencode serve`, one per workspace, reached through the executor's bridge (`session_open` starts the server under a password only the executor holds; `session_send` carries an HTTP request; the session's output carries the answers and the server's `/global/event` frames). The tests that run it:

- the driver's unit tests and the service's flow tests play an authored double of the server (`ScriptedLinker`, and the real `rca-exec` with the scripted `opencode serve` of `exec-testkit`: HTTP Basic auth, SSE, sessions that outlive a restart, the permission and question flows);
- one ignored test (`sessions.rs`, `given_the_real_opencode_when_a_session_is_opened...`) runs the executor's bridge against the real opencode 1.19.5, without a model: health, the event stream through chunked SSE, a session create, and a `noReply` prompt whose events came through the stream. It passed once by hand in the sandbox.

What nothing has seen, in the order it matters:

- **A model turn.** `message.part.delta` frames, the `pending` tool state, `permission.asked` and `question.asked` as a model's tool call emits them, `session.error` payloads, token and cost values. Their shapes are the OpenAPI document's and T3's recordings of 1.14.39 (`opencode-server-protocol.md`).
- **A steer.** The adapter sends a second `prompt_async` and keeps the turn open until the next `session.status idle` once the server has stored the message (it counts the user messages it sees against the prompts it sent). How 1.19.5 orders a prompt that arrives in the middle of a step, and whether a steer can leave a second idle behind it, is unrecorded.
- **An abort.** The turn ends on `session.status idle` with `MessageAbortedError` read as no error. The 15-second grace kill (the whole server, as for Codex) is the backstop; an aborted subagent is not aborted with its parent.
- **What the server does with `reject`**, and whether the `question` tool is enabled for a client of `serve`.
- **Steady state**: the memory of a server with many sessions, the bridge's event stream across a long idle, and the process an orphaned server would be (the executor starts it with a parent-death signal, which the test of a killed executor shows only against the scripted one).
- **The tool server.** An opencode thread is not given the service's `ask_user`: its own `question` tool is the person's question, and a permission is the approval. `POST /mcp` is scoped to a directory, not to a thread, so one thread's token could not be kept from the others.

## Why blocked

A real turn needs a provider credential, and the sandbox has none and must not spend one.

## What unblocks it

The `Real backends` workflow's opencode leg (`tooling/real-backends/run_leg.py --leg opencode`, `rs/tests/flow/real_backends.rs`) starts the real service, the real `rca-exec` and the pinned opencode with `OPENCODE_GO_API_KEY` as `OPENCODE_API_KEY`, and runs a turn that writes a file and a follow-up on the same thread. Since the conversion that leg goes through the live session. Its first green run closes the first and the last-but-one items; a run with a question or a permission is not in it.
