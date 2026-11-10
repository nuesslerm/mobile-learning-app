# rca client: the executor channel (contract)

Note (2 Nov): the executor is the separate CLI `rca-exec`, not part of `t3 serve`, and the channel is keyed by workspace, `GET /exec/v2/executors?workspace=<id>[&executor=<id>]` (M6: the registry is keyed by executor, each holding the workspaces it made); see `architecture.md`. The action table and frames below are unchanged.

Owner direction, 2 Nov: the agent CLIs (Claude Code, Codex, opencode) run in the rca service with their credentials; `t3 serve` is the **executor** of every action a model takes, in the directory it serves, through one interface common to all models; output streams back through `t3 serve` as T3 events. Plan: `docs/plans/rca-credential-isolation/plan.md`. This file is the wire contract both sides build against.

## Actions (the common interface)

Every action is `{id, tool, args}` and is answered by `{id, ok, result}` or `{id, ok:false, error:{code, message}}`; `run_command` also streams `{id, stream:"stdout"|"stderr", data}` frames before its result. All paths are relative to the executor's root and confined to it (no `..`, symlinks resolved, a path outside is `PATH_OUTSIDE_ROOT`).

| tool | args | result |
| --- | --- | --- |
| `run_command` | `command` (shell string), `cwd?` (relative), `timeout_ms?` (default 120000), `env?` (extra vars) | `{exit_code, timed_out, truncated}`; output arrives as stream frames |
| `read_file` | `path`, `offset?`, `limit?` (lines) | `{content, total_lines}` |
| `write_file` | `path`, `content`, `create_dirs?` | `{bytes}` |
| `edit_file` | `path`, `old`, `new`, `replace_all?` | `{replacements}`; `EDIT_NOT_FOUND` when `old` is absent, `EDIT_AMBIGUOUS` when it occurs more than once and `replace_all` is not set |
| `list_dir` | `path`, `depth?` (default 1) | `{entries:[{name, kind:"file"|"dir", size}]}` |
| `search` | `pattern`, `path?`, `glob?`, `kind:"content"|"files"` | `{matches:[{path, line?, text?}], truncated}` |
| `cancel` | `id` of a running action | `{cancelled}`; kills the process group of a `run_command` or stops a `worktree_setup` |
| `stat` | `path` | `{kind:"file"|"dir", size, modified_ms}` |
| `read_chunk` | `path`, `offset?` (bytes), `length?` (at most 1 MiB, the default) | `{data (base64), offset, bytes, total_bytes, eof}` |
| `create_file` | `path`, `content?`, `create_dirs?` | `{bytes}`; `ALREADY_EXISTS` when the path exists (never opened, never followed through a link) |
| `browse` | `path` (relative to the root, or absolute inside the root, the worktree base or the allowlist), `show_hidden?` | `{path, parent?, entries:[{name, kind, path}]}`, directories first; the one action that reads outside the root, and only inside `--browse-allow` |
| `search_entries` | `query`, `limit?` (default 100, at most 500), `path?` (a directory of either place; the paths returned are relative to it) | `{entries:[{path, kind}], truncated}`; best name match first; skips `.git` and `node_modules`, follows no link |
| `git_diff` | `path?`, `base?` (a revision), `worktree?` (a worktree name: diff it instead of the root; `path` is then held to it) | `{diff}` |
| `git_worktree_add` | `name`, `branch`, `base?` | `{path, branch, head}`; creates `<worktree base>/<name>`, on a new `branch` from `base` or on the existing `branch` |
| `git_worktree_remove` | `name`, `force?` | `{removed}`; only a worktree git lists is removed |
| `git_worktree_list` | none | `{worktrees:[{path, head, branch?, is_main}]}` |
| `worktree_setup` | `name`, `commands` (shell strings), `timeout_ms?` (default 300000) | `{ok, steps:[{command, exit_code}], timed_out, cancelled}`; stops at the first command that fails; progress arrives as subscribe frames |

**Confinement (M6).** A path is relative to the root, or absolute under the root or the worktree base (`rca-exec serve --worktree-base <dir>`, a directory that neither contains nor sits inside the root). It is held to the one of the two it started in: `..`, and a symlink that leaves it, dangles, or crosses to the other, are `PATH_OUTSIDE_ROOT`. Worktree names, branches and bases are checked as plain names or revisions (nothing git could read as an option), git runs with the repository's hooks off, and no worktree action takes a path.

**Subscribe frames.** An action that reports progress (`worktree_setup`) sends `{id, seq, event}` frames, `seq` counting from 1 per request, before its result frame; the events are `setup.started {steps}`, `step.started {step, command}`, `step.output {step, stream, data}`, `step.finished {step, exit_code}` and `setup.finished {ok}`.

The set is the working set of the first adapter; further tools (for example per-provider extras) are added here with their functional tests before an adapter may expose them.

## Channel

- **t3 serve to rca**: `GET /exec/v2/channel?thread=<id>` with `Authorization: Bearer <T3 bearer>`, upgraded to a WebSocket. Text frames, one JSON object each. rca sends requests (`{id, tool, args}`); `t3 serve` replies with the result frames above. One channel per thread; a second open for the same thread replaces the first (the first is closed with code 4001).
- **A turn**: `turn.start {thread, workspace, driver, model?, prompt}` of `agent.threads` on the agent socket (`architecture.md`), whose events are the thread's neutral events. It fails with `NO_EXECUTOR` if no channel is open for the workspace. `POST /exec/v2/turns` and the whole of `/exec/v1` (turns, interrupt, catalog, `fs/*`) were retired in M3 and answer 404; an interrupt is `turn.interrupt`.
- **MCP bridge in rca**: for the turn, rca serves an MCP endpoint at `POST /mcp/<thread>` on loopback only (streamable-HTTP JSON-RPC: `initialize`, `tools/list`, `tools/call`). `tools/list` returns the table above; `tools/call` forwards over the thread's channel and returns the result as MCP content (`run_command` output concatenated, exit code in `isError`). The CLI is started with its built-in file and shell tools disabled and this server configured (Claude Code: `--mcp-config` + `--strict-mcp-config` + `--disallowedTools`; opencode: `mcp` and `tools` config; Codex: `-c mcp_servers.*` and its shell tool off, to be confirmed by the spike).
- A channel that closes mid-action fails the pending actions with `EXECUTOR_GONE` and ends the turn with an `error` event.

## Functional tests (no contract-only tests)

- Executor (host side, real temp directory): each tool does the thing; confinement refuses `..` and a symlink out; `run_command` streams, times out and cancels; `edit_file` ambiguity.
- Bridge (rca side, real service process): a scripted MCP client calls `tools/call` and the effect lands on the **executor's** directory and not rca's; no channel gives `NO_EXECUTOR`; closing the channel mid-command gives `EXECUTOR_GONE`. The turns of these flows start with `turn.start` on the agent socket (`backend/remote-chat-agent/rs/tests/flow/exec_remote.rs`, and `agent_threads.rs` with the real `rca-exec`).
- End to end: a real `claude` or `opencode` with a live model, in the CI cli-e2e tier, asked to create a file and run a command, ends with the file in the `t3 serve` directory and T3 events for each step. A flow-tier stand-in uses a scripted CLI that speaks MCP.

## Nx seeding: the VM replays, the gate runs (owner: the VM must run barely anything)

- Every new crate gets its own Nx tasks automatically through `tooling/nx/cargo-crates.js`: `cargo-unit` (inline tests), `cargo-flow` (`tests/flow.rs`), `cargo-contract`, and for live tests `cargo-e2e`. The pre-push gate (`wf check --pushed`, all stages, `cli-e2e-tests` included) runs every one of them in the sandbox with the shared cache writable, so the VM replays the passes.
- The functional live-CLI tests live in a crate with `tests/e2e.rs` and `[package.metadata.e2e] groups = ["claude", "opencode", "codex", ...]` (the shape `clis/offload/Cargo.toml` uses): one `cargo-e2e-<group>` task per provider, keyed by its own test file, the shared support files and the adapter sources it exercises, so a change to one adapter reruns only that provider's tests. Each test retries up to three times like `clis/offload/tests/e2e/parity.rs` (`retrying`).
- Keep the crates small and layered (executor core, local adapter, channel client, T3 translation) so a change reruns only the crates above it; a feature must not live inside one big crate whose every edit invalidates every task.
- Done means: `wf test drift` shows the same task keys sandbox and runner, the gate's output says the stage was seeded, and the CI run of the push replays the new tasks (`wf: nx ledger` shows hits, not runs).
