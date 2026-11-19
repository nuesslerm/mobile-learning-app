---
title: rca — the service, the executor at the workspace host, and the phone mappers
date: 2026-11-19
category: architecture
module: backend/remote-chat-agent
problem_type: architecture_pattern
component: development_workflow
severity: medium
tags: [architecture, rca, rca-exec, t3-serve, orca-serve, agent-api]
applies_when:
  - "deciding where a piece of agent work runs (service, executor or mapper)"
  - "adding a phone app, a backend CLI or an executor action"
  - "setting up a machine whose project the phone should work on"
related_components: [tooling]
---

# rca: one service, executors at the workspace, mappers for the phone apps

Four kinds of process, each in one place:

```
phone (T3 app)   ──▶ t3-serve   ──┐
phone (Orca app) ──▶ orca-serve ──┴──▶ rca-service ◀── rca-exec (at the project) ──▶ claude / codex / opencode
                     (server side)      (server side)      (workspace host)            (workspace host, native tools)
```

| Process | Runs | Owns | Never |
| --- | --- | --- | --- |
| `t3-serve` (`clis/t3-serve`) | server side, beside the service | T3's phone protocol (pairing, its RPCs and events), mapped to `/agent/v1`; our stand-in for the real `t3 serve` | files, CLIs, credentials |
| `orca-serve` (`clis/orca-serve`) | server side | Orca's E2EE WebSocket RPC, mapped to `/agent/v1` | files, CLIs, credentials |
| `rca-service` (`backend/remote-chat-agent`) | server side (the VM) | the agent API `/agent/v1`: threads, turns, queue, history, approvals and questions, git, terminals, catalog; the executor registry | a project's files; it never runs a turn's CLI |
| `rca-exec` (`clis/rca-exec`) | the machine that holds the project | the executor contract: files, git, commands, ptys, `cli_detect`, and starting the turn's CLI in the project folder | thread state |

## A turn

1. The phone sends a message; the mapper calls `turn.start {thread, workspace, driver, model?, prompt}` on `rca-service`.
2. The service builds the CLI's command for the driver (`command_for_host` in `cli-driver`) and asks the executor attached for that workspace to run it (`run_command`, streamed).
3. `rca-exec` starts `claude`, `codex` or `opencode` in the project's root with the host's own credentials. The CLI edits files and runs commands with its own tools, on that machine.
4. Its output streams back; the service turns it into neutral events (`message.delta`, `tool.*`, `file.changed`, `turn.finished`) with a per-thread `seq` and pushes them to the subscribed mapper, which renders them as the app's events.
5. Approvals and questions (`approval_prompt`, `ask_user`) reach the service through its tool server `/mcp/<thread>`, which a remote host reaches with `AGENT_PUBLIC_URL` and the turn's bearer.

Why the CLI runs at the host (owner decision, 18 Nov; `docs/plans/rca-client/architecture.md`): intercepting a CLI's own tools does not work across the three CLIs (an ACP route was probed and does not intercept; Claude Code has no remote tool backend), so the CLI runs where the files are and the service only orchestrates.

## Words

- **Workspace**: a name for one project root, not a machine. One `rca-exec serve --workspace <id> --root <dir>` process serves one workspace; one machine can run several, and several machines can attach to one service. The executor id defaults to the workspace id (`--executor` sets another).
- **Driver**: `claudeAgent`, `codex` or `opencode`; `thread.create` and `turn.start` name it, with an optional `model`. `agents.detect` lists a driver's CLI only as the executor's `cli_detect` reports it.
- **Mapper**: a phone-protocol server over the agent API. A new phone app is a new mapper; nothing in `/agent/v1` is shaped to one app.

## Running it

- On the project machine: install and sign in the agent CLIs, then `rca-exec serve --rca-url <service> --workspace <id> --root <dir>` with the service's bearer in the environment (`--secret-env`). Binaries are the `wf-latest` release.
- On the server: `rca-service`, `t3-serve`, `orca-serve` (the compose services of the rca project). The phone pairs with a mapper in its app.

## Where it is proved

- `rs/tests/flow/agent_threads.rs`: a turn over the socket, the real `rca-exec` and a scripted CLI started at the host's root.
- `rs/tests/flow/real_backends.rs` with `.github/workflows/real-backends.yml`: one real turn per CLI (pinned binary, its one credential) writes a file at the host and answers.
- `clis/t3-serve/tests/integration`, `clis/orca-serve/tests/integration`: the whole chain from the phone protocol down to `rca-exec`; their traces are the coverage evidence of `docs/plans/rca-parity/coverage/`.

## Open

- Provider logins (`provider.auth.*`) still run at the service, writing `/data/providers/<p>`; a turn uses the host's own login. Moving the login to the host needs a pty-driven login and a host-side credential probe (issue #83, R17 unit 3).
