# Workspace crossings: the worklist for versioned libraries step (2)

Source: `wf check --workspace-crossings` on this commit (issue #364 phase 2, step 1). A crossing is a path dependency whose target is outside the consumer's Cargo workspace (a workspace is the directory of a `Cargo.toml` with `[workspace]`; a standalone package is its own). The check fails on a crossing missing from `clis/pipeline/data/workspace-crossings.json` and on a listed one that no longer crosses, so the list only shrinks. Today it holds 142 crossings (consumer manifest to dependency directory) onto 25 dependency directories. Nothing here is migrated.

## Reading the table

Consumers is the number of manifests that declare the dependency across the boundary, counting every member that inherits a `[workspace.dependencies]` path entry. Rows are ordered by that count, the order to migrate: a library with many consumers pays back first, and `agent-api-proto` sits under all the others.

Served by the registry: `service-kit`, `telemetry` and `rca-core` (with `rca-ladder`) are released to `mla` at 0.1.1, and every service, the clis workspace and the vnc-auth-capture listener take them by `version` and `registry`. They produce no crossing as a dependency of a consumer. The one remaining crossing among them is `service-kit -> telemetry`, a library naming its own sibling by `path` beside `version` and `registry` (allowed by the `versioned-library-path-deps` budget); `rca-core` has none.

## Crossings by consumer count

| Consumers | Dependency | Consumer workspaces (manifests) | Why it is a path dependency | Served by the registry |
| --- | --- | --- | --- | --- |
| 24 | `libraries/agent-api/agent-api-proto` | backend/remote-chat-agent (14), clis (10) | the wire types and method table every agent.* group and every client shares | no |
| 13 | `libraries/agent-api/exec-proto` | backend/remote-chat-agent (12), clis (1) | the executor action contract between rca and the executors | no |
| 9 | `libraries/agent-api/exec-git` | backend/remote-chat-agent (7), clis (2) | executor implementation crate of the agent-api contract set (`libraries/agent-api`), consumed by rca and rca-exec | no |
| 8 | `libraries/agent-api/agent-api-git` | backend/remote-chat-agent (4), clis (4) | agent.* group contract of the agent-api set (`libraries/agent-api`), consumed by the rca groups and the t3 and orca clients | no |
| 8 | `libraries/agent-api/agent-api-state` | backend/remote-chat-agent (3), clis (5) | agent.* group contract of the agent-api set (`libraries/agent-api`), consumed by the rca groups and the t3 and orca clients | no |
| 8 | `libraries/agent-api/exec-fs` | backend/remote-chat-agent (6), clis (2) | executor implementation crate of the agent-api contract set (`libraries/agent-api`), consumed by rca and rca-exec | no |
| 6 | `libraries/agent-api/agent-api-terminal` | backend/remote-chat-agent (3), clis (3) | agent.* group contract of the agent-api set (`libraries/agent-api`), consumed by the rca groups and the t3 and orca clients | no |
| 5 | `libraries/agent-api/agent-api-agent` | backend/remote-chat-agent (3), clis (2) | agent.* group contract of the agent-api set (`libraries/agent-api`), consumed by the rca groups and the t3 and orca clients | no |
| 5 | `libraries/agent-api/agent-api-attachments` | backend/remote-chat-agent (3), clis (2) | agent.* group contract of the agent-api set (`libraries/agent-api`), consumed by the rca groups and the t3 and orca clients | no |
| 5 | `libraries/agent-api/agent-api-hosted` | backend/remote-chat-agent (3), clis (2) | agent.* group contract of the agent-api set (`libraries/agent-api`), consumed by the rca groups and the t3 and orca clients | no |
| 5 | `libraries/agent-api/agent-api-workspace` | backend/remote-chat-agent (3), clis (2) | agent.* group contract of the agent-api set (`libraries/agent-api`), consumed by the rca groups and the t3 and orca clients | no |
| 5 | `libraries/agent-api/exec-hosted` | backend/remote-chat-agent (3), clis (2) | executor implementation crate of the agent-api contract set (`libraries/agent-api`), consumed by rca and rca-exec | no |
| 4 | `libraries/agent-api/agent-api-accounts` | backend/remote-chat-agent (2), clis (2) | agent.* group contract of the agent-api set (`libraries/agent-api`), consumed by the rca groups and the t3 and orca clients | no |
| 4 | `libraries/agent-api/agent-api-catalog` | backend/remote-chat-agent (3), clis (1) | agent.* group contract of the agent-api set (`libraries/agent-api`), consumed by the rca groups and the t3 and orca clients | no |
| 4 | `libraries/agent-api/agent-api-preview` | backend/remote-chat-agent (3), clis (1) | agent.* group contract of the agent-api set (`libraries/agent-api`), consumed by the rca groups and the t3 and orca clients | no |
| 4 | `libraries/agent-api/agent-api-schedules` | backend/remote-chat-agent (2), clis (2) | agent.* group contract of the agent-api set (`libraries/agent-api`), consumed by the rca groups and the t3 and orca clients | no |
| 4 | `libraries/agent-api/agent-api-service` | backend/remote-chat-agent (2), clis (2) | agent.* group contract of the agent-api set (`libraries/agent-api`), consumed by the rca groups and the t3 and orca clients | no |
| 4 | `libraries/agent-api/exec-preview` | backend/remote-chat-agent (3), clis (1) | executor implementation crate of the agent-api contract set (`libraries/agent-api`), consumed by rca and rca-exec | no |
| 4 | `libraries/agent-api/exec-proc` | backend/remote-chat-agent (3), clis (1) | executor implementation crate of the agent-api contract set (`libraries/agent-api`), consumed by rca and rca-exec | no |
| 4 | `libraries/agent-api/exec-pty` | backend/remote-chat-agent (3), clis (1) | executor implementation crate of the agent-api contract set (`libraries/agent-api`), consumed by rca and rca-exec | no |
| 4 | `libraries/agent-api/exec-testkit` | backend/remote-chat-agent (1), clis (3) | scripted CLI, temp repo and recordings that rca, wf-t3 and orca flow tests run | no |
| 2 | `libraries/agent-api/agent-api-client` | clis (2) | blocking client of the /agent/v1 API used by the t3 mapper and orca | no |
| 1 | `clis/telemetry` | backend (service-kit) (1) | service-kit logs through it; kept as `path` beside `version` and `registry` (the library names its sibling) | yes, release `0.1.x` exists; the crossing is the declared sibling path |
| 1 | `clis/wf-kit` | clis/vnc-auth-capture/listener (1) | standalone vnc-auth-capture listener builds the wf verbs it reuses | no |
| 1 | `clis/wf-login` | clis/vnc-auth-capture/listener (1) | standalone vnc-auth-capture listener reuses the login flow | no |

## Reading the data

- Twenty-two of the 25 dependency directories are crates of `libraries/agent-api`, one workspace of 22 crates that rca (`backend/remote-chat-agent`) and the clis workspace (`t3-serve`, `orca-serve` and its `map-*` crates, `rca-exec`) both take by path. Consumer manifests per workspace: `backend/remote-chat-agent` 86 of the 142 crossings, `clis` 53, the vnc-auth-capture listener 2, service-kit 1.
- The three non-agent-api rows are the only ones outside that set: `service-kit -> telemetry` (sibling, kept), and the listener's `wf-kit` and `wf-login`.
- Releasing `agent-api` as versioned libraries in dependency order (`agent-api-proto` and `exec-proto` first, then the groups and executors, `exec-testkit` and `agent-api-client` last) removes every crossing but the three above.

## Why `remote-chat-agent` runs in the test plan when secrets, wf-bucket, wf-vmops, http or wf-testkit change

None of the path dependencies of rca reach those crates: its manifests depend by path only on its own members and on `libraries/agent-api/*`, which depend on nothing under `clis`. The edge is not a Cargo dependency at all. `backend/remote-chat-agent/rs/Cargo.toml` declares

- `[package.metadata.flow] uses = ["rca-exec", "exec-testkit", "wf-mimic"]`
- `[package.metadata.integration] uses = ["wf-mimic"]`

and `tooling/nx/cargo-crates.js` (`extra`) makes the sources of every used crate, and of the closure of its path dependencies, inputs of the `cargo-flow` and `cargo-integration` tasks of `rca-service`. `rca-exec` reaches none of the five. `wf-mimic` (`clis/mimic`) does, through its path dependency on `wf-bucket`:

| Changed crate | Chain from rca | Dependency kind |
| --- | --- | --- |
| `wf-mimic` | rca `uses` | metadata `uses` (flow and integration) |
| `wf-bucket` | wf-mimic -> wf-bucket | `[dependencies]` path |
| `secrets` | wf-mimic -> wf-bucket -> secrets (also via wf-vmops) | `[dependencies]` path |
| `cli-http` (the `http` directory) | wf-mimic -> wf-bucket -> cli-http (also via wf-vmops) | `[dependencies]` path |
| `wf-vmops` | wf-mimic -> wf-bucket -> wf-vmops | `[dependencies]` path |
| `wf-testkit` | wf-mimic -> wf-bucket -> wf-testkit (also wf-vmops, optional and dev) | `[dev-dependencies]` path |

So the answer is one `uses` entry, `wf-mimic`, plus the path dependencies `wf-mimic -> wf-bucket`, `wf-bucket -> secrets | cli-http | wf-vmops | wf-testkit` and `wf-vmops -> secrets | cli-http | wf-testkit`, all inside the clis workspace. Ending the `wf-mimic` use (replacing it by a released `wf-mimic` binary or a registry version) would cut all five triggers. The derivation is from the manifests and the Nx graph code; `wf test plan` was not run against a changed-file set here (no Nx install in this sandbox).
