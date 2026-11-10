# rca client: three parts and one abstract API

Owner direction, 2 Nov. Supersedes the placement in `protocol.md` where the executor sat inside `t3 serve`; the executor wire contract in `protocol.md` still holds.

## Parts

- **rca service** (`backend/remote-chat-agent`): hosts the agent CLIs (Claude Code, Codex, opencode) and their credentials, and the extra model APIs. Exposes the **rca agent API** (below). Invokes the executor to act on a workspace.
- **`rca-exec`** (new CLI, `clis/rca-exec`): the executor. Runs at the workspace, connects out to the rca service, and performs every action a model takes there (run a command, read, write, edit, list, search, cancel) through the executor contract of `protocol.md`. It is not part of `t3 serve` and is usable by anything that serves a workspace.
- **`t3 serve`** (`clis/t3-serve`): a mapper only. T3 requests in, rca agent API calls out; the streamed rca events come back and are mapped to T3's events for parity. It holds no executor, no file access, no CLI and no credential.
- **Other mappers** (the owner's second target is Orca, `github.com/stablyai/orca`, MIT: a React Native mobile companion that speaks a WebSocket RPC with NaCl end-to-end encryption, a per-device token from pairing and a `status.get` version handshake, protocol 3 with 2 as the minimum; its worktrees map onto the executor's `workspace`; it is built mainly around terminal streaming, so mapping rca's structured events needs the RPC list read first (`mobile/src/transport/direct-rpc-client.ts`, `mobile/scripts/mock-server.ts`, the v2 framing and the relay contract in `cloud/packages/relay-contract`), or any app that speaks a different protocol) are small adapters over the same rca agent API; the service does not change.

## The rca agent API (`/agent/v1`): a generic request/response and one socket

One WebSocket per client, `GET /agent/v1/socket`, bearer-authenticated (T3's pairing bearer; pairing is T3's, see the plan). JSON text frames.

- **Request:** `{id, method, params}`; **response:** `{id, result}` or `{id, error:{code, message}}`.
- **Event:** `{event:{seq, thread, turn, type, ...}}` with a per-thread monotonically increasing `seq`; `thread.subscribe {thread, after_seq}` replays missed events, so a dropped socket resumes.
- **Methods:** `catalog.get` (drivers, models, capabilities), `thread.create`, `thread.list`, `thread.history {thread, after_seq}`, `turn.start {thread, workspace, driver, model?, prompt, attachments?}`, `turn.interrupt {thread}`, `approval.answer {approval, decision}`, `input.answer {request, answer}`, `workspace.list` (executors currently connected).
- **Events (neutral, provider-independent):** `turn.started`, `message.delta {role, text}`, `reasoning.delta`, `tool.started {call, tool, args}`, `tool.output {call, stream, data}`, `tool.finished {call, ok, result}`, `file.changed {path, op}` (derived from write and edit results), `approval.requested {approval, tool, args}`, `input.requested`, `plan.updated`, `usage`, `error {code, message}`, `turn.finished {reason}`.
- Every method and event is functional: a method that the service does not implement is absent from `catalog.get`, never answered as supported.

## Executor attachment

- `rca-exec serve --rca-url <url> --workspace <id> --root <dir>` opens `GET /exec/v2/executors?workspace=<id>[&executor=<id>]` (the channel of `protocol.md`). Since M6 the registry is keyed by executor (`--executor`, default the workspace id), and each executor owns its root workspace plus the ones made later (`workspace.create`, `worktree.create`, routed to the executor that owns the repository); a second executor with the same id replaces the first with close code 4001 and keeps its workspaces, and a workspace another executor owns is refused with 409. `--worktree-base <dir>` widens confinement to the worktree base and `--browse-allow <dirs>` lists where `browse` may read (decision 8, proposed default awaiting owner confirmation).
- `turn.start` names the `workspace`; the service fails with `NO_EXECUTOR` if no executor is attached for it. The MCP bridge of that turn forwards the CLI's tool calls to that executor.
- The service holds no workspace and runs no turn outside this API: `/exec/v1` (its turns, `fs/*` and catalog, which read and wrote the service's own disk) and `POST /exec/v2/turns` were retired in M3, so a turn is `turn.start` and its files are the executor's (`file.list`, `file.read`, and since M6 `file.write`, `file.create`, `file.readChunk`, `file.stat`, `file.search`, `file.browse`, `turn.diff`, `thread.diff` of the `files.edit` group; `worktree.*` and `workspace.create/remove` make and forget workspaces, each owned by the executor that made it). A mapper opens the socket with a device bearer and holds no service secret: `t3 serve` reads `T3_RCA_BEARER` and names its workspace with `--rca-workspace`.

## Mapping (parity)

- `t3 serve` maps T3's thread and turn commands to the methods above and the events to T3's turn items; the conformance run of `docs/plans/t3-parity/plan.md` checks every mapped action against the pinned real T3.
- A mapper for another app is done when its conformance or functional tests pass against the same service; none of the API above is shaped to one app.

## Orca wire (read from `stablyai/orca` at `main`: `mobile/scripts/mock-server.ts`, `mock-server-rpc-handlers.ts`, `src/transport/direct-rpc-client.ts`, `rpc-envelope-fields.ts`)

- **Transport**: one WebSocket (mock default port 6768). The first frame is plaintext `{"type":"e2ee_hello","publicKeyB64":<32-byte client key>}`; the server answers `{"type":"e2ee_ready"}` (or `e2ee_error`) and derives a shared NaCl key from its own key pair, whose public half is in the pairing QR and pinned by the device. Every later frame is encrypted. The first encrypted frame is `{"type":"e2ee_auth","deviceToken":...}`, answered `{"type":"e2ee_authenticated"}`; a wrong token gets an encrypted `e2ee_error {code:"unauthorized"}` and the socket closes.
- **Request**: `{id, deviceToken, method, params}` plus any extra top-level envelope fields, passed through. Response: `{id, ok, result}` or `{id, ok:false, error:{code,message}}`; streaming methods send several frames for one request id, binary frames carry terminal bytes. After authentication the client negotiates mobile runtime capabilities (a `status.get`-style call, 5 s) before it counts as connected; `mobile-liveness-*` ids are probes and are dropped.
- **Methods the mock serves** (the host's list is longer; the mock is the part the app needs): `status.get`, `settings.get`, `settings.getTerminalQuickCommands`, `settings.updateTerminalQuickCommands`, `ui.get`, `ui.set`, `repo.list`, `repo.hooks`, `worktree.ps`, `worktree.create`, `worktree.activate`, `files.open`, `files.openDiff`, `preflight.detectAgents`, `preflight.detectRemoteAgents`, `ssh.connect`, `ssh.getState`; terminal subscribe and viewport update ride the stream registry.
- **Mapping**: `worktree.*` and `repo.*` map to executor workspaces, `files.*` to the executor's `read_file` and diff, a terminal stream to an rca thread's event stream rendered as text. The mapper owns the encryption and pairing (a per-device token, as t3 pairs); the rca service sees only the abstract agent API.

## One API for both apps: capability groups

Owner direction, 3 Nov: the rca interface serves the Orca and the T3 mobile apps generically, with full parity of the endpoints each app calls. T3 is chat-shaped (threads, turns, items); Orca is workspace- and terminal-shaped (repos, worktrees, files, terminals, settings). The agent API is therefore the union of both shapes in **capability groups**, each listed in `catalog.get {groups:[...]}` only when it is fully implemented. A mapper declares the groups it needs and refuses to start (not degrade) against a service that lacks one.

| Group | Methods and events | T3 uses | Orca uses |
| --- | --- | --- | --- |
| `agent` | `thread.*`, `turn.*`, `approval.answer`, `input.answer`, the neutral events above | threads, turns, items, approvals | an agent run shown as a terminal or a row of `preflight.detectAgents` |
| `workspace` | `workspace.list`, `workspace.create {repo, name, base}`, `workspace.activate`, `workspace.remove`, `repo.list`, `repo.hooks`; events `workspace.changed` | project and worktree picker | `repo.list`, `repo.hooks`, `worktree.ps/create/activate` |
| `files` | `file.read`, `file.diff {workspace, path?, base?}`, `file.list`; run at the executor through `read_file`, `list_dir` and a git diff tool | file mentions, diff view | `files.open`, `files.openDiff` |
| `terminal` | `terminal.open {workspace, cols, rows}`, `terminal.write`, `terminal.resize`, `terminal.close`; binary output frames; replay by byte offset | none today | the terminal screen (`terminal subscribe`, viewport update) |
| `state` | `state.get/set {scope, key}` per paired device (quick commands, UI state, settings) | its settings | `settings.*`, `ui.get/set` |
| `service` | `service.status` (versions, protocol, groups, uptime), liveness ping | `server.getConfig` | `status.get` and the capability negotiation |

- **Executor additions**: `workspace`, `files` and `terminal` run at the workspace, so `rca-exec` gains `git_worktree`, `git_diff` and `pty` actions beside the existing six (contract in `protocol.md`, one flow test each against a real git repo and a real pty). Nothing in the service touches a workspace filesystem.
- **Mappers own their wire**: `t3 serve` keeps T3 pairing and event shapes; a new `clis/orca-serve` owns the NaCl handshake, device token and RPC envelope and maps each Orca method to one group method. Neither wire leaks into the service; the service has one auth (a bearer per paired device) and one event model.
- **Parity as a table**: each mapper ships a generated coverage table (app method -> group method or "not offered"), and a test fails when the app's method list (T3: the pinned conformance goldens; Orca: the mock server's handler list at a pinned commit) names a method with no row. `ssh.*` is "not offered" and stays out of `catalog.get`.
- **Built (3 Nov, later)**: `files` (`file.read`, `file.list`, `file.diff` over the workspace's attached `rca-exec`; `git_diff` is its new action) and `workspace.list` (the attached workspaces; the rest of the `workspace` group is not built, so the group is not listed). `t3 serve --rca-url` asks `service.status` at startup and refuses another protocol or a missing group.
- **Built (3 Nov)**: `service` (`service.status`: protocol 1, version, commit, groups) and `state` (`state.get`, `state.set` by device scope and key) on `GET /agent/v1/socket`, dispatched by `rca_agent_core::AgentApi`, the groups in `rca-service-core` (`application::agent_groups`) and registered by `rca_service::agent_api`; the flow test `rs/tests/flow/agent_api.rs` drives a real socket.
- **Build order**: `agent` (in progress) -> `service` and `state` (small, unblock both handshakes) -> `workspace` and `files` -> `terminal` (the largest: a pty at the executor, byte-offset replay, resize). Each group lands with functional tests through a real `rca-exec` and the scripted CLI, and seeds the Nx cache.

## A real Orca host as the parity oracle (researched 3 Nov, not yet run)

- Orca ships a headless host: `orca serve` (Linux `orca-ide serve`) runs orcad, a pure-Node server with no display or GPU, `--port`, `--json` and `--mobile-pairing` (prints the pairing URL). Upstream CI builds orcad (`pnpm build:orcad`) and runs it on ubuntu-24.04-arm without xvfb. Read at stablyai/orca `9f1b14d9` (v1.4.214).
- Needs Node 24 and pnpm 12.14.0 (the sandbox has 22 and 10.28) and a C++ toolchain for the patched `node-pty` and `cpu-features`. The install footprint is unmeasured; the sandbox's 1.7 GB free is likely too little, so the oracle runs on the VM runner.
- Oracle test: start orcad and `orca serve` side by side, pair the same test client with both, run the E2EE v2 handshake (`e2ee_hello` -> `e2ee_ready` -> `e2ee_auth` -> `e2ee_authenticated`), call `status.get` and every method the mapper offers, and diff the responses (ids and timestamps normalised). The method list comes from the host's `defineMethod` registry at the pinned SHA, so a new upstream method fails the coverage table.
- Fallback when orcad cannot run on the VM: the owner pairs a phone to a real host once and the decrypted frames are pinned as goldens; they go stale per Orca release, so the SHA is pinned with them.
- First step: on the VM runner, `pnpm build:orcad` and Orca's `smoke:orcad-terminal`, measuring disk and time.
