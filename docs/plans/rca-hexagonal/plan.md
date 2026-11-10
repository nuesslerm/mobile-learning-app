# remote-chat-agent (rca): hexagonal architecture, crate split and test optimisation

Status: **steps 1, 2, 3a and 3b done** (8 Nov 2026: the render crate exists, core owns `Opencode`, `Commands`, `Outbound`, `Ledgers`, `DocumentText` and `SpeechToText`, the generic doubles live in core `testing`; `application` moved to `service-core`); 3c is done (the golden log-line flow test `rs/tests/flow/log_lines.rs` and typed `log::info!("event", key = value)` fields; the run-time-built meta of `voice::plan_log` and `one_shot` still uses the slice functions); step 4 is done but for the real-process flows (see its status line), step 5 is done but for a flow over `MemoryFiles` and the move of `Host` (step 7c); steps 6 to 9 are open. Written 2 Nov 2026. Written from a read-only investigation of the tree; every claim below was checked
against the tree on 2 Nov unless marked **unverified**. It replaces the gap note `docs/gaps/rca-layers-without-ports.md`
(folded in here and removed). No behavior change in any step: the flow tests are the parity oracle.

Related: `docs/solutions/architecture/hexagonal-architecture-by-package-type.md` (type 1, Rust service),
`docs/plans/hexagonal-architecture/spec.md` (Phase 4, R1 to R9), `docs/plans/wf-crate-split/plan.md` (the split this one copies),
`docs/plans/t3-parity/plan.md` (the T3 phases that add provider adapters to rca).

## 1. Where rca stands

`backend/remote-chat-agent` is one Cargo workspace (`Cargo.toml`, members `core` and `rs` only) and one pnpm package
(`@app/remote-chat-agent`, which also holds the TypeScript e2e and bench suites under `tests/`).

| Ring | Where | Lines |
| --- | --- | --- |
| core crate `remote-chat-agent-core` | `core/src`: domain 1,872, application 799, exec 411, testing 33 | 3,125 |
| `rs/src/api` (axum routes, ws, wire, `app.rs`) | `rca-service` | 3,808 |
| `rs/src/application` (runs 722, one_shot 462, edit_stats 302, voice_planner 226, attachments 173, opencode_client 72, ports, startup) | `rca-service` | 2,120 |
| `rs/src/domain` (voice_dsl 717, schema, util, clis, models, md) | `rca-service` | 1,085 |
| `rs/src/infrastructure` (15 modules; opencode_backend 319, tamil 238, stt 192, highlight 139) | `rca-service` | 1,546 |
| flow tests `rs/tests/flow/*.rs` (14 modules + harness), plus `integration.rs`, `deployed_verification.rs` | | 4,454 for `rs/tests` |

Also: `stt/` (python sidecar), `opencode-server/` (container). Rust does not call Docker. The line counts above match the
inventory given for this plan.

### Two things are called "rca core"

- `backend/remote-chat-agent/core`, crate `remote-chat-agent-core` (unpublished): the thread engine and exec. This is the
  service core of a type-1 package.
- `libraries/rca-core` and `libraries/rca-core/plugins/ladder` (`rca-core`, `rca-ladder`, 0.1.x, published to the `mla` registry,
  `libraries.json`): the run, ladder and file-store logic that `wf` and rca-service share. rca-service depends on them by
  registry version (`rs/Cargo.toml`).

Decision: keep `remote-chat-agent-core` as the service core and never reuse the word for adapters. Adapter crates are named
`remote-chat-agent-{opencode,host,speech,render}`. The Phase 4 spec still says `clis/rca-core` (lines 193, 267 to 277, 568 to 570);
the library lives at `libraries/rca-core`. Step 8 corrects the spec.

### Open debt (what the gap note tracked, plus the rest of type 1)

- R3 and R9 are done: `Clock` and `IdSource` exist (`core/src/domain/ids.rs`), the opencode `Client` is
  `rs/src/application/opencode_client.rs` over the `Opencode` port, `Commands`, `Outbound` and `Ledgers` are in
  `rs/src/application/ports.rs`; `WorkingTree`, `Backend`, `Store`, `Spawner`, `Workspace` are in core.
- `application` still reaches `infrastructure::{docs,log}`, and the glob ignore `remote-chat-agent/rs/src/application/*.rs`
  (`backend/arch-rules/application-does-not-reach-outward.yml`) hides it.
- Twelve `api` files are on the ignore list of `backend/arch-rules/api-does-not-use-infrastructure.yml`: `app.rs`,
  `routes/{chat,convert,diff,edit_stats,inventory,mod,runs,tamil,voice,worktree}.rs`, `ws/mod.rs`.
- `api/app.rs` (`App::new`, `orchestrators`, lines 113 to 159) builds the clock, ids, `GitWorkingTree`, `FileStore`,
  `OpencodeBackend` and `DiffStore`: a second composition root beside `main.rs`.
- Env is read in every ring: 53 `env::var`/`std::env` hits across 22 files by `grep` (the investigation counted 47 sites with an
  unknown method; the exact count is **unverified**, the order of magnitude is not). The flow harness sets about 40 vars under
  a global mutex (`rs/tests/flow/harness.rs:40`, `set_var` at :57 and :66).
- Process, fs and clock sites outside adapters: `application/runs.rs` has 24 `Command`/`fs` sites (the investigation said 31
  counting env as well), `api/routes/diff.rs` 7, `file_preview.rs` 2, `voice.rs` 2, `worktree.rs` 1, `edit_stats.rs` 1.
- Tests sit almost entirely in flow. The real adapters (`proc`, `exec`, `working_tree`, `file_store`, `stt`, `docs`,
  `Http` in `opencode.rs`) have no unit or contract tier of their own.

## 2. Target

```
remote-chat-agent-core     domain + application + every port + testing doubles
remote-chat-agent-render   pure compute: md, highlight, tamil, docx text
remote-chat-agent-opencode Http adapter, OpencodeBackend, loopback fake (feature testing)
remote-chat-agent-host     Processes, LocalFs, ShellCommands, GitWorkingTree, FileStore, RunHost for rca-ladder,
                           DiffSnapshots, DocumentText
remote-chat-agent-speech   SpeechToText adapter (stt sidecar client)
rca-service (rs)           api + three tiny infrastructure adapters (outbound, ords, system) + main.rs/compose.rs, the one root
```

- **core** gains the ports now in `rs/src/application/ports.rs` (`Opencode`, `Commands`, `Outbound`, `Ledgers`), and new ones:
  `DocumentText`, `SpeechToText`, `WorkspaceFiles`, `DiffSnapshots`, `Settings` (typed, read once at the root). The doubles of
  all ports ship in core's `testing` feature (as `wf` crates do). `rs/src/application` and `rs/src/domain` (3,205 lines) move into
  core in step 6. Core stays pure: serde, serde_json, time, tokio (no `fs`, `process`, `net`), async-trait, futures-util, tracing.
- **render** takes the pure compute that today drags tree-sitter, quick-xml, resvg, image, pulldown-cmark and fancy-regex into
  `rs`: `domain/md.rs`, `infrastructure/{highlight,tamil}.rs`, docx text from `infrastructure/docs.rs`. A change to routes no
  longer recompiles the grammars.
- **opencode** takes `infrastructure/{opencode,opencode_backend}.rs` (387 lines) and a loopback fake HTTP server for contract tests.
- **host** takes `proc`, `exec`, `working_tree`, `file_store`, the fs parts of `docs`, and the `RunHost` implementation for
  `rca-ladder` (R8). It is the only crate that spawns processes or touches the workspace fs.
- **speech** takes `stt.rs` (192 lines) with the whisper and ffmpeg process wiring.
- All new crates are service-internal: `publish = false`, path dependencies, not in `libraries.json`. `rca-core` and `rca-ladder`
  are hoisted into the workspace `[workspace.dependencies]` so each crate names them once. They stay registry dependencies.

Constraints that shape the steps:

- New crate dirs sit at the same depth as `rs/` (`include_bytes!` paths in `rs/src` are relative; the Tamil font and
  `mocks/code-assist-responses.json` are copied into the Docker context at fixed paths, Dockerfile lines 39 to 40).
- Each member is added by name to `backend/remote-chat-agent/Cargo.toml` (no globs) and gets a `COPY` line in the Dockerfile
  `planner` stage (lines 36 to 38 today copy `core` and `rs`); a missing line fails the cargo-chef prepare. The image hash
  (`clis/pipeline/src/image_hash.rs`: Dockerfile, the context files it copies, the Cargo.lock entries of the crate closure) changes on
  the first push of each crate, so rca redeploys once per such push (2 to 6 minutes by the investigation, **unverified** here).
- Never bundle a `service-kit`, `telemetry` or `rca-core` release in a step: the 1 Nov service-kit/telemetry push redeployed all six
  services in 55 minutes (reported by the investigation, **unverified** here).

## 3. Nx granularity: the catch

`tooling/nx/cargo-crates.js` `packageOf` walks up from a crate directory to the first `package.json` and `projectOf` folds the crate
into that package. `backend/remote-chat-agent/package.json` exists (the TS e2e and bench suites), so `core` and `rs` are already
one Nx project, `@app/remote-chat-agent`; a new crate under `backend/remote-chat-agent/` joins it too. The investigation says its
inputs are `{projectRoot}/**/*` plus `^rustProduction`; I did not read the target definitions that set them, so that detail is
**unverified**, but the folding is verified.

Consequence: splitting crates gives compile parallelism and smaller link units, but **no rerun granularity** while the
TypeScript package sits at the service root. `wf` got granularity because `clis/*` crates have no `package.json` above them.
Granularity needs step 9 (Option A): move the TypeScript package to `backend/remote-chat-agent/tests/` (it already holds
`bench`, `blackbox`, `e2e`, `fixtures`, `helpers`; the three vitest configs and `tsconfig.json` move with it), so the crates become Nx
projects of their own and `cargo-unit`, `cargo-flow`, `cargo-clippy` and `cargo-e2e-<group>` tasks key on each crate.

Name references to `@app/remote-chat-agent` that step 9 must keep working (change the name of the moved package only if the
references move in the same push): `clis/deploy/src/{report_run,verify_tier3}.rs`, `clis/deploy/tests/flow/check.rs`,
`clis/pipeline/src/legs.rs`, `clis/pipeline/tests/flow/{expected_revisions,nx_cache}.rs`, `tests/e2e/globalSetup.ts`,
`core/src/domain/types.rs`. Decision: the moved TS package keeps the name `@app/remote-chat-agent`; the Rust project gets the
crate names. The deploy legs that key on the name (`legs.rs`) then decide whether they mean the image or the TS suite; check each
site, do not assume.

## 4. Steps

Each step is one push to `dev`, changes no behavior, and is rolled back by reverting its commit. Verify every step with
`pnpm run arch:services`, rca flow (`test:flow`) and integration (`test:integration`), run through the pre-push gate
(`wf check --pushed`). Never edit a flow assertion to make a step pass.

1. **Render crate.** Move `md`, `highlight`, `tamil` and the docx text extraction into `remote-chat-agent-render`; `rs` calls it.
   First crate: it proves the Cargo.toml member, the Dockerfile `COPY`, the image hash and the `include_bytes!` depth once, and
   measures the link and `target/` cost (section 6).
2. **Core owns the service ports.** Move `Opencode`, `Commands`, `Outbound`, `Ledgers` (and their types) from
   `rs/src/application/ports.rs` to core `application/ports.rs`; the doubles in `rs/tests/flow/doubles.rs` that are generic move to
   core `testing`. This is the first shrink of the application glob ignore.
3. **Three ports.**
   - 3a `DocumentText` port; `infrastructure/docs.rs` becomes its adapter. Removes `application -> infrastructure::docs`.
   - 3b (done, uncommitted 2 Nov) `SpeechToText` port, `Transcription` and `SttError` in core `application/ports.rs`; `stt.rs` is its adapter `Speech`; `api/routes/voice.rs` calls `app.speech` and no longer names `infrastructure::stt` (`voice_planner` never did).
   - 3c (partly done, uncommitted 2 Nov) `infrastructure::log` already writes through `tracing`; it moved unchanged to `application::log` (the layers' logging seam), so the 9 route/ws ignores and the application glob ignore are gone. Done 8 Nov 2026: the golden log-line test (written first, against the old output) and `application::log` typed fields (`log::info!`, `log::warn!`, `log::error!` macros over the same slice functions; lines byte-identical). Logging: `infrastructure::log` (25 lines) is replaced by `tracing` in the layers, with a golden log-line test first (the
     line a call produces is what the sink returns; the repo's logging rule applies). Removes `application -> infrastructure::log`.
4. **Typed `Settings` and one composition root.** First a round-trip test: every env name and default the service reads today
   (about 50 sites) produces the same `Settings` value. Then `Settings::from_env` is the only reader, `main.rs`/`compose.rs` build
   everything, and `api/app.rs` stops constructing adapters (it takes `Ports` and `Settings`). The flow harness's env list
   shrinks per module and its mutex goes (spec R6 and R7). Env names and defaults do not change.
   - Status 8 Nov 2026 (step 4 done except the one item below, staged): the round-trip tests (`rs/tests/flow/settings.rs`, written first against the old readers, and `Settings::names()`) pass; `Settings::from_lookup` is the only reader of the service's variables. `main.rs` builds `Documents` (`Documents::new(commands, &Settings)`), `Speech`, the opencode `Client`, and, through `rs/src/compose.rs` (`opencode_client`, `orchestration`, `thread_engine`), the orchestrator's `FileStore`, `SystemClock`, `RandomIds` and `OpencodeBackend`; they arrive in `Ports` (`client`, `orch`, `gateway: Arc<dyn Gateway>`, the new port the routes that name a session directly call). `App::new` builds no adapter. `docs` and `opencode_backend` take a `Settings` value; no adapter names `ProcessEnv` any more but `Speech`, which takes the `Environment` port and reads `settings().stt` at each recording, because the voice flow changes `STT_URL`, `GROQ_API_KEY` and `WHISPER_*` while one service runs (a snapshot would need that flow rewritten). The flow harness's `Ports.env` is a map double (`rca_service_core::testing::MapEnv`, new `testing` feature) and `Env::new` shares no process state, so those flows no longer take turns. Open: the flows that run real children or a real executor (`agent_accounts`, `agent_attachments`, `agent_lifecycle`, `agent_queue`, `agent_requests`, `log_lines` (its sink is process-wide), `agent_git`, `agent_preview`, `agent_schedules` and `agent_threads` (every flow over the real `chain`), whose guard changes `PATH`) read the process environment through `exec::inherited()` or the executor library, so they use `Env::process`, which keeps the global mutex; it goes when `Processes` takes its inherited variables as a value. `exec::inherited()` and the `main.rs` heartbeat/healthcheck arguments stay outside `Settings`.
5. **Filesystem behind ports.** `WorkspaceFiles` (file preview, convert, worktree removal), `DiffSnapshots` (`DiffStore`,
   `routes/diff.rs`), and the `RunHost` implementation for `rca-ladder` that `application/runs.rs` uses today through 24
   process and fs sites (spec R8). Each port gets a double in core `testing`.
   - Status 8 Nov 2026 (staged): the tree is ahead of this step's text. `WorkspaceFiles` exists in core (`application/ports.rs`) with the adapter `HostFiles` (`infrastructure/workspace_files.rs`), and file preview, convert, tamil, worktree removal and `application/runs.rs` all go through it and `Commands`: a grep of `service-core/src` and `rs/src/api` finds no `std::fs`, `tokio::fs`, `Command::new` or `tempfile` site. `DiffSnapshots` has nothing to wrap: there is no `DiffStore` and no `routes/diff.rs` in the tree (the diff viewer was removed earlier), so the port is dropped from the target list. The `RunHost` implementation for `rca-ladder` is `Host` in `service-core/src/application/runs.rs`, written over `Client`, `Commands`, `Ledgers` and `WorkspaceFiles`, with no process or fs call of its own. Done now: the core `testing` double `MemoryFiles` for `WorkspaceFiles` (with unit tests). Open: no flow runs over `MemoryFiles` yet (the flows use `HostFiles` on a temp dir, which they inspect on disk); moving `Host` into `remote-chat-agent-host` is step 7c, since it needs that crate.
6. **Move `rs/application` and `rs/domain` into core.** Delete the application glob ignore and the 12 api ignores; the rules then
   have an empty `ignores` list. The wire layer in `api/wire.rs` stays in `rs`. Core stays pure (step 8 enforces it).
   - Status 8 Nov 2026 (done, staged): `application` and `domain` live in `rca-service-core` (`service-core/src`), which depends on `remote-chat-agent-core`; `rs` declares neither module and `rca-service-holds-adapters-only` forbids them. The last ignore, `api/app.rs` in `api-does-not-use-infrastructure`, is gone: `api` reaches the loopback MCP address, the executor sockets and the agent socket through the transport ports in `rs/src/ports.rs` (`McpAddress`, `ExecutorSocketPort`, `AgentSocketPort`), which `infrastructure` implements and `agent_api()` registers. `api-does-not-use-infrastructure` has no `ignores` key. The ports name axum types, so they stay in `rs` and not in core, which stays free of axum. Not moved into `remote-chat-agent-core` itself: the two core crates stay (rca-service-core holds the service's use cases, the thread engine stays in `remote-chat-agent-core`); merging them is not required by the exit condition.
7. **Adapter crates.**
   - 7a `remote-chat-agent-opencode`: `Http`, `OpencodeBackend`, loopback fake in its `testing` feature.
     - Status 8 Nov 2026 (done, staged): the crate is `backend/remote-chat-agent/opencode` (`Http`, `OpencodeBackend`, `EventMapper`; `testing` feature: `Loopback`, an opencode server on a loopback port that records requests and serves scripted `/event` data; 5 unit tests drive `Http` and the backend against it). `rs` depends on it (`main.rs` builds `Http`, `compose.rs` the backend) and `infrastructure/opencode*.rs` are gone. Registered in the Cargo workspace members and `Cargo.lock`, the Dockerfile planner `COPY`, `package.json` `test:unit`, `.ls-lint.yml` and `blast-radius.json` (each crate below `rca-service-core` gains one dependant, so their budgets rose by one; computed by hand from `cargo metadata`, `wf check --blast-radius` was not run). The flow harness keeps the in-memory `FakeOpencode` of core `testing`, so no flow assertion or harness line changed; no flow runs over `Loopback` yet. Open: the crate depends on `rca-service-core` for `Client`, `Settings`, `Gateway` and the log macros (an adapter over the core, as the step 8 rule allows).
   - 7b `remote-chat-agent-speech`.
     - Status 8 Nov 2026 (done, staged): the crate is `backend/remote-chat-agent/speech` (`Speech`, `Engine`, `resolve_engine`; the faster-whisper sidecar, Groq and whisper.cpp behind ffmpeg, behind the `SpeechToText` port). `rs/src/infrastructure/stt.rs` moved into it unchanged; `rs` depends on it (`main.rs` and the flow harness build `Speech`). It has no `testing` feature: the flows keep driving `Speech` over the core doubles `FakeOutbound`, `FakeCommands` and `MapEnv`, so no flow assertion or harness line changed beyond the import path. 7 unit tests (Given/When/Then) drive engine resolution and `Speech` over those doubles (they did not exist for `stt.rs`). Registered in the Cargo workspace members and `Cargo.lock`, `rs/Cargo.toml`, the Dockerfile planner `COPY`, `package.json` `test:unit`, `.ls-lint.yml` and `blast-radius.json`. Open: like 7a, the crate depends on `rca-service-core` for `Environment`, `SttSettings` and the log macros.
   - 7c `remote-chat-agent-host`. Land it after the T3 phase that touches `/exec/v1` is merged, because that phase adds
     provider-CLI adapters to the same files and the move would conflict.
     - Status 8 Nov 2026 (staged): the T3 phase is merged (`/exec/v1` is retired), so the move is cut. The crate is `backend/remote-chat-agent/host` (`System` for `Commands`, `Processes` with `supervise` and `provider_command` for the CLI driver's `Spawner`, `HostFiles` for `WorkspaceFiles`, `FileStore` for the thread `Store`, `Documents` for `DocumentText`, and `Hosts`, the `RunHost` of `rca-ladder`). `Host` could not move as it was: `runs::run` built it and the crate depends on `rca-service-core`, which would be a cycle. `runs.rs` now defines `HostSetup`, `AttemptHost` (`RunHost` plus the diff, session, events and usage of an attempt) and the port `RunHosts`; `Ports.hosts` carries it, `run` takes it, and `Host` with the CLI choice, the tool-event folding and the diff stat lives in the crate (`Hosts` is its `RunHosts`). `rs` depends on the crate; `main.rs` and the flow harness build `Hosts`, `HostFiles`, `Documents` and `System` from it, so no flow assertion changed. 7 unit tests (Given/When/Then) over the real `System`, `HostFiles` and `FileStore` on a temp dir. Registered in the workspace members, `Cargo.lock`, `rs/Cargo.toml`, the Dockerfile planner `COPY`, `package.json` `test:unit`, `.ls-lint.yml` and `blast-radius.json`. Not moved, because the plan does not name them for 7c: `infrastructure/{blobs,provider_dirs,provider_login,previews}.rs` (`provider_login` now calls the crate's `supervise` and `provider_command`) and the thread `working_tree` (no such file in the tree). Open: no flow runs `Hosts` over `MemoryFiles` (step 5's open item stays); the crate depends on `rca-service-core` for `Client`, `Settings`, `one_shot` and the log macros, like 7a and 7b.
8. **Arch rules and docs.** An ast-grep rule that adapter crates name no other adapter crate and no `api`; an ls-lint rule for the
   crate layout; a core-purity budget in `clis/pipeline/src/core_purity.rs` (no `fs`, `process`, `net`, system clock, randomness in
   core); close the debt in `docs/solutions/architecture/hexagonal-architecture-by-package-type.md` (type-1 row, the "Debt" and
   "Holes today" lines); update the Phase 4 status and the stale `clis/rca-core` paths in `docs/plans/hexagonal-architecture/spec.md`;
   remove the ignore comments' pointer to this plan once the lists are empty.
9. **Optional: per-crate Nx tasks.** Move the TS package under `tests/` (section 3), then add the rca equivalent of the wf test
   `a_change_in_a_wf_crate_plans_the_flow_tests_of_exactly_the_crates_that_build_it`
   (`clis/pipeline/tests/flow/nx_cache.rs:472`). Do this last, after the crate boundaries have stopped moving.

## 5. What reruns, today and after

Today, any file change under `backend/remote-chat-agent` (a route, a Dockerfile comment, a TS test) reruns the whole
`@app/remote-chat-agent` project: unit, flow, clippy, integration and the e2e group that follows (inputs as reported by the
investigation, **unverified** in the target definitions). The `Dockerfile` hash and the crate closure decide the deploy leg
separately (`image_hash.rs`), and that does not change.

After step 9 (not before):

| Change in | Reruns |
| --- | --- |
| `render` | render unit and clippy; flow (rs depends on it); not opencode, host or speech tests |
| `opencode` | its contract tests and clippy; flow |
| `host` | its contract tests and clippy; flow; integration |
| `speech` | its contract tests; the voice flow module through the one flow binary |
| `core` | everything that depends on it (all crates): the intended blast radius |
| `rs` (api, compose) | rs unit, flow, integration, clippy |
| the TS suites only | the e2e and bench tasks, no Rust |

The per-group e2e tasks already exist for offload (`cargo-e2e-ask`, `-parity`, `-parity_features`, `-parity_capabilities`, defined
in `tooling/nx/cargo-crates.js`); rca can use the same `e2e` group metadata for its TS-free Rust e2e if any appears. Today rca
has none; this is not planned.

## 6. Test optimisation

- **Adapter contract tests, one set per crate** (the missing tier): `host` against a tempdir and a real child process
  (`Processes`, `LocalFs`, `GitWorkingTree`, `FileStore`); `opencode` against its loopback fake; `speech` against a stub binary on
  `PATH`; `render` as plain unit tests (pure). Each is Given/When/Then and asserts behavior (what a file looks like after a write,
  what a spawn returns on a non-zero exit), not that a name is set.
- **Flow stays one binary**, `rs/tests/flow.rs`, with the 14 modules as the parity oracle. Moving a port does not move its flow
  test; a double moves to core `testing`, the assertions do not change.
- **Golden JSON for `core/src/domain/types.rs`**, shared by the zod schemas in the TS suites and serde in Rust, so a wire change
  fails in both places and the frontend cannot drift.
- **Live-model e2e only after steps 6 and 7c** (they move runs and opencode, the paths a live model exercises); not after each step.
- **Pre-push gate** `wf check --pushed` seeds the Nx cache for the whole package before each push, so CI replays rather than
  recompiles on the VM. Target per push: under 10 minutes warm (investigation figure; **unverified**).
- **Disk and link cost rule** (from the wf split, `docs/plans/wf-crate-split/plan.md` section 7): record flow-stage time and
  `target/` size before step 1, after step 1 and after step 7c. If either grows past the budget set in step 1 (write it in the commit
  message; the wf split used a stop rule), merge `speech` into `host`. Disk, not CPU, is the bottleneck of a VM test run
  (`backend/remote-chat-agent/Cargo.toml` profile comment).
- **Doubles, not mocks of project modules**; every new port ships its double in core `testing`.
- Reports and recordings: any run that produces a flow or browser report publishes its bundle (`wf test publish`) as the repo
  rule requires; this plan adds no new harness.

## 7. Risks and open questions

1. **Crate names.** Recommendation: `remote-chat-agent-core` stays; adapters `remote-chat-agent-{opencode,host,speech,render}`;
   never `rca-core` for anything new (it is the published library). Cheap to change before step 1, expensive after.
2. **Step 9 last.** Moving the TS package changes the Nx project name and the references in section 3; doing it before the crate
   boundaries settle repeats the work. Recommendation: last, optional, and only if the measured rerun cost justifies it.
3. **One host crate now.** Recommendation: one `remote-chat-agent-host`; split out `-providers` when it passes about 1,500 lines
   (the T3 provider adapters will push it there; a figure from the investigation, **unverified**).
4. **`rca-file-store` library extraction** (spec: the 56-line file store leaves `rca-core`, which claims no I/O but calls
   `std::fs`). Recommendation: a separate track. It is a breaking 0.2.0 of `rca-core` and rebuilds both `wf` and rca; keep it out of
   every step here (never bundle a library release). Until then `host` re-exports nothing and calls the library's `FileStore`.
5. **Sequencing 7c after the T3 provider-adapter phase.** Recommendation: steps 1 to 6 and 7a, 7b now; 7c after that phase merges,
   so the host crate is cut once with provider CLIs in it. If that phase slips past step 6, do 7c first and rebase the phase onto it.

## 8. Not verified

- The measured costs: redeploy time per push (2 to 6 min), the 55 minute figure of 1 Nov, link and `target/` sizes, the under-10-minute
  warm push.
- The Nx project inputs of `@app/remote-chat-agent` (`{projectRoot}/**/*`, `^rustProduction`): the package-folding was read in
  `cargo-crates.js`, the target definitions were not.
- The exact number of env read sites (53 by grep; 47 reported) and runs.rs's 31 (24 process/fs sites by grep).
- Whether every item of the target ports list fits the current call shapes; step 2 and 5 may find a port that needs two traits.
- Nothing was built, compiled or run for this plan; no gate was run.
