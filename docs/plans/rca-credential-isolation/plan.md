# rca runs the agent CLIs; `t3 serve` implements their API and executes the tools where the work is

Status: proposed, 2026-11-02 (owner direction, revised twice that day; this is the third and current form). Companion to `docs/plans/github-access-broker/plan.md`. Owner: keep auth as simple as possible; isolation is the point; no TTLs, leases or credential distribution; pairing is exactly T3's (API parity).

## Roles

- **The rca service** hosts the provider CLI processes (Claude Code, Codex, opencode) with their credentials, the compatibility layers (the T3 protocol) and the extra APIs that use the models (voice transcription and the like). It is the only place a provider credential exists.
- **The workspace host** is the separate rca workspace-host container (today `remote-chat-agent-opencode`, which already mounts `/workspace`), and later any machine. `t3 serve` runs there. Bash, edits and writes happen there, so a turn behaves like a local CLI run on that directory, with the model and the CLI hosted by the service.
- **`t3 serve` is the executor of every action a model may want to take** (owner, 2 Nov): the tools the three CLIs actually use (run a command, read, write, edit, list, and the search tools Claude Code and opencode have as separate tools; Codex searches through the shell), the exact set fixed by the spike from the union of the CLIs' tools and T3's item types. Stopping a turn (`thread.turn.interrupt`) is control, not an action: it kills what `t3 serve` is running for that turn and tells the CLI. It exposes them through **one common interface that every model uses**, the same for Claude, Codex and opencode, and it performs them in the directory where it runs. It speaks T3's protocol to the T3 apps, runs no CLI, holds no model and no credential.
- **In rca, each model CLI is reached through a proxy, meaning only the code that sits between `t3 serve` and that CLI** (not a network gateway for model traffic): an adapter that gives that CLI the common executor interface in the form the CLI supports (the CLI's built-in file and shell tools off, the executor offered in their place, for example as an MCP tool server or as the client-side file and terminal requests of the Agent Client Protocol) and passes each tool call and result between the CLI and `t3 serve`. The three adapters differ; the interface they front does not. Adding a model means writing one adapter, not another executor.
- **No credential ever leaves the service.** The workspace host holds none of the three providers' credentials; it holds the shared VM GitHub token (like agent-shell) for git, and nothing a provider CLI can read.

## Where it stands

- The three CLIs share `HOME=/workspace/home` across the agent, opencode and agent-shell containers (`backend/remote-chat-agent/docker-compose.prod.yml:69,227`); `CLAUDE_CODE_OAUTH_TOKEN` sits in the agent container's env (`:192`) and `/exec/v1` spawns the CLIs with that env inherited (`rs/src/api/routes/exec.rs`); Codex's `auth.json` is under the shared `CODEX_HOME` (`:199`); opencode has its own keys in its container env (`:46-60`). Every session can reach every provider's credential.
- Today the CLIs also run on the service's own disk: `/exec/v1/turns` runs the CLI inside the service, and `/exec/v1/fs/*` reads and writes the service's `/workspace` (`clis/t3-serve/src/adapters/rca.rs`). `wf t3 serve`'s location changes nothing about where files change.

## Design

### A. Isolation inside the service

- One Unix user per provider (`claude`, `codex`, `opencode`), each with its own home, mode 0700; each CLI's files and token live only there.
- The service runs as root, or with `CAP_SETUID` alone, and spawns each CLI with `Command::uid/gid/groups`, `env_clear()`, and only that provider's `HOME`, `TMPDIR`, `PATH` and variables (replacing `/exec/v1`'s inherited env). Children run with `no_new_privs` and a dropped capability set; `/proc` is `hidepid=2`.
- The entrypoint writes the env tokens into each home as 0600 and unsets them, so the service's own process holds none after start.
- The voice transcription and other model APIs run as their own user with only the key they need.

### B. The work at the workspace: a remote tool channel

- The CLI runs in the service but its file and shell tools execute at the workspace host, through `t3 serve`, over the common executor interface. The candidate mechanisms for each CLI's adapter are an MCP tool server with the CLI's built-in tools disabled, or the Agent Client Protocol: the agent (the CLI behind its ACP adapter) asks the client for `fs/read_text_file`, `fs/write_text_file` and `terminal/*`, and `t3 serve` is that client at the workspace. Unverified, and the first landing is a spike to settle it: whether each of the three CLIs' adapters routes all of its edits and shell commands through the client or runs some tools itself; a CLI that does would have to run at the workspace host instead, with its credential bound for that one process (the fallback, per provider).
- Pairing is T3's pairing, exactly (`docs/plans/t3-parity/protocol.md` section 3): `wf t3 pair` / `POST /api/auth/pairing-token` (12 characters, 5 minutes, single use), `POST /oauth/token` for a bearer, T3's revocation. The conformance run covers it. The tool channel is an rca extension on the same bearer and is listed in `tooling/t3-conformance/allowlist.json` with its reason.

### C. What the service stops doing

- It no longer reads or writes a workspace: `/exec/v1/fs/*`, the worktree routes, `WORKTREE_BASE_DIR`, the `/workspace` mount and its git credential go away.

## Parity and function (owner, 2 Nov)

- **Everything streams through `t3 serve`.** The adapters hand a CLI's output (message deltas, tool calls and results, command output, file changes, approvals, plans, errors) back to `t3 serve`, which turns it into T3's events. Parity with T3 Code covers every agent action, not a subset.
- **No empty API.** A route or RPC that `t3 serve` exposes must do the thing. Today every RPC outside the Phase 1 set answers `UnsupportedOperationError` (`wf t3 --help`); for agent actions the target is zero of those, and any that remain are named in `tooling/t3-conformance/allowlist.json` with a phase, never answered as if supported.
- **Functional tests, not only contract tests.** The conformance goldens and the scripted T3 client prove shapes; they do not prove an edit happened. For each agent action in T3's set (turn start and resume, streamed assistant message, command execution, file read, write and edit, approval request and answer, interrupt, plan and todo items, error turns) there is a functional test, Given/When/Then through the T3 protocol: Given a real temporary workspace and a real agent CLI behind its adapter, When the T3 client sends the action, Then the effect is on disk or in the process list (the file contains the text, the command ran and its output streamed, the interrupt stopped it) and the events that arrived match T3's shape. A negative case for each proves the request reached the host (a refused approval changes nothing).
- **Tiers, per `docs/solutions/testing/verification-ladder.md`:** flow tests keep the CLI doubled and are fast, so they run on every push; the functional tier drives the real CLIs with a live model, retried like the live-model e2e suite (`WF_PUSH_CLI_E2E`), seeded into the shared Nx cache by the gate so CI replays it, one Nx task per provider so a change to one adapter reruns only its tests. A route with a flow test and no functional test is not done.
- **Live parity check against the real T3 server** is the conformance run of `docs/plans/t3-parity/plan.md` (pinned and cached tarball, diff of transcripts), run for every adapter's actions, not for the Phase 1 set only.

## Landings (each: test first, one push, CI under 20 min)

1. **Spike: remote tool execution per provider** (read-only findings doc): for Claude Code, Codex and opencode, does the ACP route cover every file and shell tool? Output: a table, and the fallback chosen per provider.
2. **Per-user homes and a clean spawn env** in the service. Test: a Codex turn cannot read Claude's token file and its env has none of Claude's variables; the same the other way and for opencode.
3. **The service holds no provider env after startup** (0600 files, `/proc/self/environ` clean).
4. **The executor and the first adapter**: `t3 serve` at the workspace host executes the actions for the first provider the spike cleared, and the output streams back as T3 events. Functional test: a T3 turn edits a file and runs a command in `t3 serve`'s own directory and not in the service's, with the streamed events asserted.
4b. **Each further adapter, with its functional tests, before it counts as supported.**
5. **The workspace host runs `t3 serve`** (the existing workspace-host container); the service drops `/workspace` and the file routes (section C).
6. **Git at the workspace host** with the shared VM token; `GITHUB_TOKEN` is gone from the vault and the registry since 13 Nov (broker plan); `VM_PROVISION_GITHUB_TOKEN` is the one GitHub credential.
7. **The voice API and other model APIs** under their own user.

## Not done and risks

- The boundary inside the service is kernel user separation, not namespaces; separate containers per provider stay the fallback.
- If a CLI cannot be driven through a remote tool channel, it runs at the workspace host with its credential bound to that process, which is the leak this plan avoids; that is why the spike is first.
- Codex's refresh token rotates in the service's single home, so there is one writer and the multi-serve problem does not arise.
- Open: the exact name of the tool-channel route (it must not collide with a future T3 route: check the next pinned T3 release's route table); the rename of `wf t3` to follow the product (`wf rca`), the owner's call.
