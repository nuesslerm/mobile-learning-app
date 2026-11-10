# Hexagonal architecture: one dependency rule, a shape per package type

> 23 Oct 2026: the `wf-core` and adapters target of type 3 (Phase 6, the wf units W1-W9, and open question 12 below, including W2's in-process `wf::run` seam) is superseded by `docs/plans/wf-crate-split/plan.md`, done on 24 Oct: wf is split into `clis/wf-*` crates along its seams, Cargo's dependency graph is the layer wall, and each crate's flow tests drive its thin binary through argv.

Owner request, 21 Oct 2026: "could you create another spec to apply clean hexagonal architecture to every package? possibly different architecture in different types of packages, but we should document this. and clean up the existing clean arch doc"

This file is the plan. It names every package type, the target shape of each type and the type of every package. It sets the order of the migration and says how each rule is proven.

The rule itself lives in [the architecture doc](../../solutions/architecture/hexagonal-architecture-by-package-type.md), `docs/solutions/architecture/hexagonal-architecture-by-package-type.md`. That doc was written beside this file on 21 Oct, and it replaces the archived "Clean architecture standard" (see [The doc cleanup](#the-doc-cleanup)). Its shape names are the ones used here. The doc says what the rule is; this file says how each package gets there. A unit that changes a rule or a check updates both.

`file:line` references are to HEAD `bec59f1ed` (21 Oct 2026), before the Phase 0 edits drafted beside this file; those edits shift some lines (`standard-crates.md:95` and `:101` become `:97` and `:103`). Counts come from the package inventory or from the command named beside them. The per-type assessments of that day were not committed; a figure taken only from them is marked "unreproduced".

## Context

### What the documents say today

As committed on the morning of 21 Oct, before the Phase 0 edits drafted beside this file.

| Source | What it says | True today? |
|---|---|---|
| `.agents/instructions/root.md:19-21` (rendered into CLAUDE.md, "Code architecture") | Feature-first; `features/<name>/{domain,data,state,auth,ui}`; `ui → integration → state → data → domain`; import a feature only through its barrel; leetcode is the reference; enforced by `pnpm --filter mla-tamagui run arch` | Partly. leetcode has no `domain/`. `auth/` exists only in leetcode, `integration/` only in coding-agent. `hooks/`, `design-system/` and `config/` have no files in `frontend/mla-tamagui`, and `app-shell/` is not listed. 29 `app/` files re-export a `routes/` module: 18 deep-import one (15 from in-app features, 3 from `components/dashboards`), and 11 go through the `./routes/*` exports of the coding-agent, service-report and settings packages. The `arch` script cruises only `features/leetcode` and runs only by hand. Nothing about services, CLIs, libraries or tooling. |
| `.agents/instructions/frontend/mla-tamagui/features.md:5-21` | Two feature forms; a package checks itself with oxlint and dependency-cruiser | Partly. The lists match, and the four packages that run dependency-cruiser pass. "One barrel" ignores the `./routes/*` and `./testing` exports. dev-tools and settings have no domain; service-report's `use-cases/` and `lib/` sit in no layer. |
| `CONCEPTS.md:147-151` "Feature Triad" | contracts, `-impl` and `-mocks` sibling packages | Stale: none exist, and root.md:21 forbids them. There is no entry for port, adapter or composition root, and two flow-test entries (`:91`, `:141`). |
| `docs/archive/solutions/architecture/clean-architecture.md` (208 lines) | contracts ← data ← use-cases ← delivery ← di for every package; two-tier DI; Nx `enforce-module-boundaries`; "contracts, not domain" | Mostly stale. No Nx boundary rule exists since `923d53408` (4 Oct) removed it. Most of the shapes it describes are gone, though several packages it names remain (leetcode-backend, agent-config-sync, secrets, registry-service, logging). The Rust services use domain/application/infrastructure/api. Two-tier DI was built in the TypeScript packages and removed with them between 4 and 9 Oct; no current package uses it. The dead-refs sweep dcab33aae archived it on 19 Oct with no successor. Its 43-line twin `clean-architecture-standard.md` is archived beside it. |
| `docs/plans/rust-services/standard-crates.md:30-39, 101` | Hexagonal core/rs crate pairs; ls-lint and ast-grep; `ignores` as a ratchet | Holds for `backend/`: `pnpm run arch:services` exits 0 (run 21 Oct). Its status names three of the five services; reporting-service and watchdog are missing. It accepts that wf has no outbound-HTTP port. |
| `docs/plans/greenfield/requirements.md:269` | "The slim wf … ≤ 15k LOC, hexagonal, one esbuild bundle" | Stale. wf is a Rust crate with 87 flat top-level `.rs` files and 34,876 lines under `clis/wf/src` (`git ls-files`, 21 Oct). |
| `docs/solutions/testing/verification-ladder.md:58-59` | Unit tests never import `data/` or `di/`; a flow test boots `di/dataContainer.ts` and `useCasesContainer.ts` | Stale. The named lint rules and containers do not exist. leetcode has unit tests on `data/` and `di/`. |
| `docs/solutions/testing/flow-tests-mount-whole-routes.md:21,59,64,66` | Override table; fakes from "the -mocks sibling"; a convention rule for route re-exports | Partly. The override table holds. The `coding-agent-impl` paths and the `-mocks` sibling are gone, and the route rule lost its enforcer (`docs/plans/after-device-lab/spec.md:42`). |
| `docs/gaps/feature-di-migration.md` | Every feature follows leetcode's ports and DI; `ui` never imports `data` | The debt is real. Its frontmatter paths `features/*-impl/src/**` (:6-9) and its links (:13-14) are stale. Its target conflicts with feature-kit, which allows `ui → data` (`frontend/feature-kit/depcruise.feature.cjs:7`). |
| `docs/gaps/leetcode-ui-direct-io-debt.md` | A MermaidDiagramPort and a catalog port | The debt is real. Its verify (:12) exits 0 although the debt remains. |
| `docs/plans/rca-hexagonal/plan.md` | remote-chat-agent's use cases and routes call adapters; the ast-grep `ignores` are the ratchet | The debt is real. `links: []` (:11) names neither this plan nor the doc, and its "Done when" covers only the ignores, not the core clock and ids, the second composition root or the env reads. |
| `docs/plans/ts-removal/plan.md:39` | leetcode-backend shows all four layers | Wrong: `backend/leetcode-backend/core/src` holds `application/` only. |

### What enforces it today

| Rule set | Covers | Runs in | Gap |
|---|---|---|---|
| Cargo crate wall | the five Rust service core/rs pairs | every build | Keeps axum and reqwest out of a core, but not `std::fs`, `std::env` or `std::process`. tokio's `fs`, `net` and `process` features are also open: the workspace declares them (`backend/Cargo.toml:31`), and the remote-chat-agent, reporting-service and watchdog cores inherit them. |
| ast-grep `backend/arch-rules/` (4 rules) and `.ls-lint.yml` | `backend/*/core`, `backend/*/rs` | pre-commit `service-architecture`, glob-gated (`lefthook.yml:41-44`); manual `architecture.yml:111-112` | Not in CI. The rules match a path that starts `crate::<layer>` or `super::<layer>`, so `use crate::{api, infrastructure::log}` and `super::super::infrastructure` pass (the services assessment proved this with `ast-grep test` on a scratch copy). The ignores name 12 api files (`api-does-not-use-infrastructure.yml:9-20`) plus the glob `remote-chat-agent/rs/src/application/*.rs`. |
| dependency-cruiser via `frontend/feature-kit/depcruise.feature.cjs` | `@feature/{coding-agent,dev-tools,service-report,settings}` | each package's `lint`, so CI's typecheck-lint-unit-tests stage (`clis/wf/src/test_plan.rs:19`) | Not pre-commit. `wf check --staged` finds a staged file's package through the hand list `WORKSPACE_PARENTS` (`clis/wf/src/verify.rs:285`). That list has no `frontend/mla-tamagui/features` or `frontend/libraries` entry, so the file goes to mla-tamagui's lint, which ignores those packages (`frontend/mla-tamagui/package.json:12`). The preset allows `ui → data`, and files outside a layer are free (`depcruise.feature.cjs:1-8`). |
| dependency-cruiser `frontend/mla-tamagui/.dependency-cruiser.cjs` | `features/leetcode` only | the `arch` script (`package.json:13`), run only by `architecture.yml:108-109` | That workflow is `workflow_dispatch` only (`:16-17`) although its header says "every push / PR" (`:3`). `domain-imports-contracts-only` is vacuous because leetcode has no `domain/`. The header cites the archived doc's old path (`:7`). |
| oxlint `frontend/.oxlintrc.json` | frontend | `lint` | Its import bans (`:58-84`) are bundle-size and platform-behaviour bans, none of them layer rules. The layer rule `no-direct-io-in-feature-ui` and the thin-route rule were dropped. |
| nothing | `features/tamil-writing`, `components/dashboards`, `frontend/libraries/*`, `libraries/*`, `tooling/*`, `clis/*`, `backend/service-kit`, `infra/` | — | 15 of the 35 pnpm projects have no `lint` script (`pnpm ls -r`, 21 Oct): leetcode-auth-cookie-capture, remote-chat-agent-contracts, reporting-service, service-kit, vnc-auth-capture, app-config, app-di, gesture-geometry, registry-service, rn-agentation, voice-edit-dsl, vm-ops-api, watchdog, eslint-plugin-mobile-learn and hook-runtime. |

CI runs neither clippy nor the inline unit tests of any CLI crate. wf's `Cargo.toml:11` sets `test = false`, and the plan schedules only each crate's `--test flow` (`test_plan.rs:199`).

### Why one rule does not fit every package

- **Some packages have nothing to port.** gesture-geometry has no imports at all; voice-edit-dsl imports only zod. A port there would be an interface with one implementation.
- **Some packages are the outer ring of another package's hexagon.** cli-http, cdp and telemetry are adapters; their consumers define the ports.
- **Some files hold no rule.** Terraform, compose files and the Nx plugins have no decision a unit test could pin.
- **Some packages need the full hexagon.** The services, wf and the app features meet several outside systems. wf alone spawns processes from 96 `Command::new` sites in 48 files (`git grep`, 21 Oct).
- **Package walls hold only in Cargo.** `shamefullyHoist: true` (`pnpm-workspace.yaml:120`) lets `tooling/browser-harness/ui` resolve `@cli/harness-use-cases` without declaring it. A TypeScript layer is therefore a folder checked by lint, not a package.

So this spec keeps one dependency rule for every package and lets the shape vary by type.

## The model

### The dependency rule

Every dependency points inward:
- **The core does no I/O.** The core is the domain plus the application. It touches no file, network, process, environment, wall clock or randomness. It may log, through `Logger.*` in TypeScript or `tracing` in Rust; that is the one sanctioned ambient effect.
- **The application declares a port for each outside capability it needs**, named in its own words (`SessionStore`, `MetricsStore`, `LeetCodeGatewayPort`), never the transport's ("run curl").
- **A driven adapter implements one port over one technology.** A driving adapter (HTTP router, CLI parser, React screen, Lua binder) turns input into a use-case call and renders the result.
- **The composition root is the only module that names a concrete adapter.** It reads configuration once and builds the adapters.
- **The package that owns a port ships its double**, in its own testing module. There are no `-mocks` sibling packages.
- **A package has only the rings it needs.** A ring nobody needs is not created.

### One vocabulary

The same words name the rings in every hexagonal type. Only the directory differs.

| Term | Rust service | Hexagonal CLI | Hexagonal feature | Hexagonal library | Small tool |
|---|---|---|---|---|---|
| domain | `core/src/domain/` | `wf-core/src/<family>/` | `domain/` | core dirs | `src/core/` |
| application (use cases) | `core/src/application/` | `wf-core/src/<family>/` | `state/` (hooks, stores) | core dirs | `src/core/` |
| ports | `core/src/application/ports.rs` | `wf-core/src/ports.rs`, `<family>/ports.rs` | `domain/ports.ts` | a `ports.ts` in the core | parameter types in `src/core/` |
| driven adapters | `rs/src/infrastructure/` | `wf/src/adapters/` | `data/` | `adapters/` or the mapped dir | `src/adapters/` |
| driving adapters | `rs/src/api/` | `wf/src/cli/` | `ui/`, `routes/` | the React layer | entry files, `src/ui/` |
| composition root | `rs/src/main.rs` | `wf/src/world.rs`, `main.rs` | `di/` | the host app | entry files at the `src/` root |
| doubles | `core/src/testing.rs` (feature `testing`) | wf-core `testing` (feature) | `testing/` | `src/testing/`, `./testing` export | `src/testing/` |
| walls | Cargo, ast-grep, ls-lint | Cargo, ast-grep, ls-lint, budget | dependency-cruiser, oxlint | dependency-cruiser, oxlint | oxlint or ast-grep, ls-lint |

Where ports live is the one deliberate difference.
- In Rust an adapter may import the application ring (infrastructure → application), so ports sit in `application/`.
- In a feature, `state/` holds React hooks and zustand stores, and an adapter must not import React. So ports sit in `domain/ports.ts`, and `data/` imports `domain/` only.
Both follow the same sentence: the core declares the port, in the innermost ring the adapters may import.

### Choices this spec makes

1. **One rule; the layout varies by type.** Each type below gives its layout.
2. **Layers are folders in TypeScript and crates in Rust where the wall pays.** Folders are checked by dependency-cruiser and oxlint, Rust modules by ast-grep. No TypeScript package per layer.
3. **Every rule set has a planted-violation test and runs from the owning package's `lint`.** That puts it in CI (typecheck-lint-unit-tests). At pre-commit, `wf check --staged` runs the `lint` only when a JavaScript or TypeScript file is staged (`clis/wf/src/verify.rs:167-169,364-365`), so Rust sources, ast-grep rule files and rule tests reach pre-commit only through lefthook's `service-architecture` glob job (`lefthook.yml:41-44`), which this plan widens. The planted-violation test must count its cases, because `ast-grep test` passes with zero tests. No architecture rule lives only in a manual workflow.
4. **Debt is a ratchet.** dependency-cruiser known-violations baselines and ast-grep `ignores` name files, never globs, and only shrink. Each migration unit deletes its own entries. A `wf check` rule enforces this (unit 2.8).
5. **Device services are not ports by default.** Linking, the clipboard, expo-updates, expo-constants and audio playback get a port only when a test must drive or observe them. So do native views (WebView, Skia). The flow harness stubs only native runtimes: reanimated, gesture-handler, screens, safe-area, bottom-sheet, markdown, victory-native, Skia, expo-router and its drawer, expo-audio, expo-font and AsyncStorage (`frontend/libraries/flow-test-harness/src/vitest/flowTestConfig.ts:37-69`). It has no stub for Linking, the clipboard, expo-updates or expo-constants: in a flow test they run through react-native-web or the module's own web build, and react-native-webview is the app's real iframe component (`frontend/mla-tamagui/vitest.flow.config.ts:17-23`).
6. **A feature store may persist through zustand `persist` and AsyncStorage in `state/`.** The harness stubs AsyncStorage (`flowTestConfig.ts:67-68`).
7. **Configuration is read once, in the composition root.** In Rust that is a clap derive with `env =`. In the app it is one object of literal `process.env.EXPO_PUBLIC_X` reads in the app's composition root, passed to `@lib/app-config`'s pure resolver, because Expo inlines only static member reads.
8. **Out of scope:** error types (`Result<_, String>` against thiserror) and LOC budgets.

## Package types

### How to tell a package's type

Ask in order; the first yes decides.

1. Is it a declarative file a tool reads (Terraform, compose, a lint config, a JSON registry)? → **16 Config and data**.
2. Does it only sequence other programs, where wf cannot run or is not built yet? → **15 Glue**.
3. Is it deployed and does it run on its own (an image or a deploy leg)? → **1 Rust service**.
4. Is it a CLI with rules and several outside systems? → **3 Hexagonal CLI**.
5. Is it a tool whose rules meet one or two outside things (the filesystem, a host API, a page)? → **4 Small tool**.
6. Is it a Rust library of rules shared by several consumers? → **5 Core library**.
7. Is its whole job I/O against one outside system? → **6 Adapter library**.
8. Is it `frontend/mla-tamagui` outside `features/`? → **7 App shell**.
9. Is it a feature a user reaches? If it calls a backend or persists data → **8 Hexagonal feature**; otherwise → **9 Presentation feature**.
10. Does it hold only schemas and types that two or more packages consume? → **10 Contracts package**.
11. Is it a frontend or JS library? With no I/O → **11 Pure library**; with rules that wrap I/O → **12 Hexagonal library**; rendering components → **13 Component library**.
12. Does it drive other packages' code under test? → **14 Test harness**.

Each type takes one of the six shapes the architecture doc defines (its "Shapes" list):

| Shape (architecture doc) | Types here | Rings |
|---|---|---|
| Full hexagon | 1 Rust service, 3 Hexagonal CLI, 8 Hexagonal feature, 12 Hexagonal library | domain, application with its ports, driven and driving adapters, composition root, doubles |
| Functional core, imperative shell | 4 Small tool | one core (domain and application merged) whose ports are its parameters, adapters, entry files as composition roots |
| Core only | 5 Core library (also each service's `core` crate and wf-core), 10 Contracts package, 11 Pure library | pure rules, port traits where a consumer supplies the I/O, doubles |
| Adapter only | 6 Adapter library | flat modules over one outside system |
| Delivery only | 7 App shell, 9 Presentation feature, 13 Component library, 14 Test harness | driving adapters; where the package wires others, its composition root and the adapters it hands to library ports (the shell's `app/_layout.tsx` and `lib/`, the harness's vitest config and stubs) |
| None | 15 Glue, 16 Config and data | none |

### 1. Rust service

Members: leetcode-backend, leetcode-auth-cookie-capture, reporting-service, remote-chat-agent, watchdog, vm-ops-api (`backend/vm-ops-api/{core,rs}`, `@backend/vm-ops-api`, written in this shape by `docs/plans/vm-ops-api-rs/plan.md`); the login gate as the crate pair `clis/vnc-auth-capture/core` and `clis/vnc-auth-capture/listener` (see [Phase 6](#phase-6-wf)).

Rings:
- **core crate** (`backend/<svc>/core/src`):
  - `domain/`: pure types and rules. Time and new ids come in as arguments.
  - `application/`: use cases over ports. `ports.rs` holds one trait per driven capability and the `Ports` struct of `Arc<dyn Port>`.
  - `testing.rs`, behind cargo feature `testing`: in-memory fakes of the core ports.
  - Dependencies are pure crates only: serde, thiserror, async-trait, time, futures, tracing, and tokio for time, sync and spawn.
- **service crate** (`backend/<svc>/rs/src`):
  - `infrastructure/`: one module per port implementation. `config.rs` is the clap derive with `env =` and the only reader of configuration. An optional `ports()` helper builds the real adapters; only `main` calls it (`backend/leetcode-backend/rs/src/infrastructure/mod.rs:23`).
  - `api/`: the axum router. A handler parses, calls one application function and maps the result. `pub fn app(Ports, Settings) -> Router` is shared by `main` and the flow tests (`backend/leetcode-backend/rs/src/api/mod.rs:60`).
  - `main.rs`: the composition root.

Dependency rule:
- `api → application → domain` and `infrastructure → application`. `main` may use everything.
- `api` names no infrastructure and does no fs, process or env I/O.
- The inner rings (core, and any rs-crate `application/` or `domain/`) never use `std::(fs|net|process|env)`, `tokio::(fs|net|process)`, reqwest, axum, rusqlite, `SystemTime::now`, `OffsetDateTime::now_utc` or `Uuid::new_v4`. They may use `tokio::time`, `tokio::sync`, `tokio::spawn` and `tracing`; reporting-service's scheduler (`core/src/application/scheduler.rs:8,40-51`) and watchdog's sweep (`core/src/application/sweep.rs:44`) stay legal.

Variants:
- **No `domain/` until there are rules worth isolating.** leetcode-backend has application only (`standard-crates.md:35`).
- **No `api/` without an inbound protocol.** watchdog's `main` drives the application loop.
- **An adapter may define a narrower transport seam for its own flow test.** Example: watchdog's `DockerCommand` (`backend/watchdog/rs/src/infrastructure/docker_cli.rs:22`) under the `Docker` adapter. The application never names a seam.
- **A service crate may hold `application/` and `domain/` modules** (`.ls-lint.yml:16`), only for code that needs a service-crate dependency, under the same rules.
- **vm-ops-api takes the standard pair with a `domain/`, and a composition root that does more.** `backend/vm-ops-api/rs/src/main.rs` also serves `--healthcheck` and the three helper subcommands that its sibling containers run. The generated host scripts are render functions in the `scripts/` folder of the core's `application/` ring, held to their snapshots (`core/tests/snapshots/`) by golden tests. Its ports are the table in `docs/plans/vm-ops-api-rs/plan.md`, section 4, "Ports".
- **The login gate takes the standard pair.** It is deployed and exposed on the tailnet, so its rings get the Cargo wall, not only ast-grep: `clis/vnc-auth-capture/core` holds the session engine (`clis/wf/src/vnc/engine.rs` today), its ports and a `testing` feature; `clis/vnc-auth-capture/listener` holds axum, the adapters and `main`. wf depends on the core crate, and on the listener's adapters only if it needs them.
- **Kept outside the rings:** backend/service-kit is an adapter library (type 6), used from `main`, `api` and `infrastructure`, never from a core. Single-file sidecars that adapt one library are glue: `backend/remote-chat-agent/stt/server.py` (125 lines) and `infra/oracle-cloud/functions/budget-guard/func.py` (90 lines).

Tests:
- **Unit:** core functions, and application services over core fakes.
- **Flow** (`test:flow` = `cargo test --test flow`), two shapes:
  - the real `api::app` driven over core fakes with tower `oneshot`;
  - real adapters against hermetic stand-ins: a loopback upstream, a temp dir, a SQLite file, or a fake Docker socket. A case that opens a socket is integration by `verification-ladder.md:44`, though it runs in the crate's flow binary, and is reported as integration.
  - Where a fake and a real adapter share a port, one generic test runs against both.
- **Config:** passed as a struct, never through process env. Today rca's flow harness sets about 40 env vars under a global mutex (`backend/remote-chat-agent/rs/tests/flow/harness.rs:28-58`).
- **Deployment verification:** `test:verify:deployed`.

Enforcement:
- The Cargo wall.
- ast-grep `backend/arch-rules/`, rewritten to match the layer identifier anywhere in a `use` declaration or path.
- New `inner-rings-do-no-io` and `api-does-no-io` rules.
- Named ignores only, and an `arch-tests/` file per rule run by `ast-grep test`.
- ls-lint.
- `arch:services` in each service package's `lint`.

Reference packages:
- leetcode-backend: three rings.
- leetcode-auth-cookie-capture: four rings, a fake for each of its seven ports (`core/src/testing.rs:41-152`), and two driving adapters (`main.rs:50-95`).

### 2. TypeScript service

Empty since vm-ops-api moved to Rust (`docs/plans/vm-ops-api-rs/plan.md`). The number stays so the others keep theirs.

### 3. Hexagonal CLI

Member: wf, which becomes `clis/wf-core` plus `clis/wf`.

Rings:
- **`clis/wf-core`** (new lib crate), feature-first:
  - One module per command family holds its pure rules, its use cases and its `ports.rs`. A use case is a function that takes ports and returns a typed result; it never prints.
  - Shared ports live in `src/ports.rs`: `Repo`, `Workspace`, `VmOps`, `Clock`, `Evidence`.
  - Fakes live in `testing`, behind cargo feature `testing`. The Fake device (`clis/wf/src/device/commands.rs:690`) moves there.
  - Allowed dependencies: serde, serde_json, regex, time, sha2, rca-core. Never cli-http, cdp, mlua, tokio, nix, tiny_http or axum.
  - Ports are blocking traits. wf is synchronous; tokio stays inside the vnc listener and `tailnet_forward.rs`.
- **`clis/wf`** (lib and bin):
  - `src/cli/` holds the driving adapters: routes, args, help, the `--json` envelope, rendering through an `Io` handle, `device/cli.rs`, the Lua binder, and the HTTP servers wf runs (`browser_daemon.rs`, `staticd.rs`, `stub_server.rs`).
  - `src/adapters/` holds the driven adapters, one per outside system. There is one process runner (`adapters/proc.rs`), and all HTTP goes through cli-http.
  - `src/world.rs` and `main.rs` are the composition root. They read env and `.env` once into a `Config` and build `World`, one boxed port each, constructed lazily. `main` is `std::process::exit(wf::run(argv, &World::real(), &mut Io::std()))`.

Dependency rule: `cli → wf-core ← adapters`. Only `world.rs` and `main.rs` name an adapter.

CLI variant rule: a capability gets a port only where a wf-core use case holds a decision worth a unit test: the ship, test, check, device, ask/plan and secret families. A thin command has no port. Its `cli/` handler calls the adapter that `World` built, its I/O still sits in `src/adapters/`, and the black-box integration suite covers it. This keeps a pass-through command from costing a port, a fake, a contract test and a black-box test (`standard-crates.md:101` already accepted no outbound-HTTP port for wf).

| Family | Modules | Lines | Shape | wf-core holds | Ports |
|---|---|---|---|---|---|
| ship | ship, deploy, deploy_run, build_image, ship_eas, vmops | 1,510 | hexagon | the leg plan and its reasons (`deploy.rs:349-402`), the build script (`build_image.rs`), the serving wait | VmOps, Repo, Workspace, Expo |
| test | test_plan, e2e_tests, e2e_run, test_publish, test_results, test_metrics, nx, flow_run, flow_goldens, test_goldens, run, run_report, report_run, runbook, golden, pixelmatch, video_check, lock_closure, slot, run_stop | 5,322 | hexagon | the CI plan (`test_plan.rs:225`), focused-e2e selection, lock closure, golden compare, results query, report model | Workspace, Repo, Nx, Metro, Bucket, RunsTable, Device, Clock, Evidence |
| check | verify, verify_tier3, dead_refs, report_health, visual | 2,968 | hexagon | budgets (`verify.rs:112-122`), affected packages (`verify.rs:229`), dead-ref matching, tier-3 rules | Repo, Workspace, Pnpm, Gh, VmOps, Page, Bucket |
| ask/plan | dispatch, dispatch_batch, dispatch_run, worker_plan, worker_run, cooldown_mirror, pricing_sync | 3,123 | hexagon | rca-core's run, worker rules (`worker_plan.rs:15`) | RunHost, Opencode, Repo, GateRunner, LedgerStore, RunLog |
| device | `device/` | 4,269 | hexagon, already shaped (`device/mod.rs:1-4,158`) | api table, selectors, dispatcher, golden policy | Device, Evidence, Clock |
| browser | browser, browser_daemon, browser_debug, browser_launch, browser_tunnel, trace, staticd, stub_server | 4,042 | hexagon around the daemon | `trace.rs`, pixelmatch, debugger-capture rules | Page, Launcher |
| android | android, android_input, android_u2, android_lease, android_lab, android_features | 2,027 | thin | lease rules and input planning as pure functions | none; adb and the lease store are adapters |
| secret | `secrets/`, auth | 2,460 | hexagon | registry, audit and writer rules, cache policy | Backend (`secrets/backend.rs:111`), Clock |
| login gate | `vnc/` | 1,385 | its own crate pair (type 1) | nothing: the session engine (`vnc/engine.rs`) moves to `clis/vnc-auth-capture/core` | Desktop, Relay, SecretSource, Clock, in that core |
| vm | vm_exec, vm_logs, vm_ops, vm_recover, vm_sampler, infra, tailscale*, tailnet_*, agent_deps | 2,899 | thin | `infra::free_tier` (`infra.rs:250`) and recover decisions as pure functions | none of its own (VmOps is ship's); oci, terraform and the tailnet are adapters |
| repo, bench, knowledge | bench, agent_proof, app_bench/, knowledge, knowledge_gemini, agents, cm6 | 3,117 | thin | `app_bench/domain.rs`, knowledge ranking | none; Gemini, node and the store are adapters |
| log and shell | main, routes, args, help*, json_mode, env_file, usage, log_emit, logfmt, spans, root, time, js, stop | about 1,500 | driving adapter and composition | none | — |

Line counts: `wc -l` over each family's `.rs` files under `clis/wf/src`, 21 Oct.

Tests:
- **Unit:** wf-core use cases over the fakes.
- **Contract:** one generic function per port, run against the fake and the real adapter, in `clis/wf/tests/flow/contract.rs`, a module of the flow binary, so CI (`clis/wf/src/test_plan.rs:198-199` schedules only `--test flow`) and the Phase 6 gate run it. The real adapters run against today's doubles: argv-recording stubs on PATH (`tests/flow/support/stubs.rs`) and mock services (`tests/flow/support/fake_vm.rs`).
- **Flow:** `wf::run(argv, &World::fake(..), &mut Io::buffer())`, in process, with no spawned process, socket or env mutation.
- **Integration:** today's 111 black-box tests in `clis/wf/tests/flow/*.rs` (declared by `tests/flow.rs`; `git grep -c '#\[test\]'`, 21 Oct). They drive the built binary with stubs on PATH, which makes them integration tests by `verification-ladder.md:44`. They stay the unchanged oracle until each family's flow tests cover it. After that they shrink to what only a real process proves: argv, help and `--json`, exit codes, wire formats, the daemons, and the Nx cache contract.
- **E2e:** CI only.

Enforcement:
- The Cargo wall, plus a `compile_fail` doctest in `wf-core/src/lib.rs` that imports `cli_http` and `cdp`.
- ast-grep in `clis/sgconfig.yml`: `core-does-no-io`, `wf-cli-does-no-io`, `wf-cli-uses-no-adapters`, `wf-adapters-use-no-cli`. `wf-cli-does-no-io` covers `clis/wf/src/cli/**`: no `Command::new`, `std::fs`, `std::env`, `std::net` or `cli_http`. Its ignores name only the three servers (`browser_daemon.rs`, `staticd.rs`, `stub_server.rs`) and the `Io` handle.
- ls-lint.
- A `wf check --budget` entry `wf-io-outside-adapters`: the I/O sites (`Command::new`, `std::fs`, `std::env`, `std::net`, `cli_http`) under `clis/wf/src` outside `src/adapters/`, the three servers, `world.rs` and `main.rs`. It measures porting, not file location: moving a file into `cli/` does not lower it. Its first value is measured when the budget lands (W2); the `Command::new` part alone is 96 sites in 48 files today (`git grep -c Command::new -- clis/wf/src`). Each family's unit lowers it.

### 4. Small tool

Shape: functional core, imperative shell.

Members: `@lib/hook-runtime`, `tooling/browser-harness` (collapsed to one package), provision-apps (extracted from cloud-init), and agent-config-sync (the Rust variant).

Rings, in one package:
- **`src/core/`:** the rules, with domain and application merged because a tool is too small to split them. Ports are the parameters the core asks for, typed in the core, for example `decide(payload, { rules, condition, approval, repoRelative })`. The core imports only the core and `node:path`, `node:url` and `node:crypto`. It never imports a node built-in in either form (`node:fs` or bare `fs`), `node:child_process` or React, and never reads the globals `process`, `window`, `document` or `location`, directly or through `globalThis`.
- **`src/adapters/`:** implement those parameters over the real world: fd 0, spawn, git, the approval file, `window.location`, `window.__shell`.
- **`src/ui/`:** only for a page. Components import the core only.
- **Entry files at the `src/` root:** `dispatch.mjs`, `opencode-plugin.mjs`, `steps/*.mjs`, `main.tsx`. These are the composition roots: the only files that import adapters, read argv or env, or call `process.exit` or `createRoot`. Each real entry point has one.

Dependency rule: entry files → {ui, adapters} → core, and the core → nothing outside it. The existing seams already fit: `dispatch.mjs:91` takes `run`, `guard.mjs:252` takes `condition`, `guard.mjs:322` takes `loaded`, and `opencode-adapter.mjs:102-111` takes `run` and `log`.

Rust variant (agent-config-sync):
- The pure modules (contract parsing, render, paths, the guard decision) never import the shell (`fsx`, `sync`, `cli`).
- It has no port traits: a temp checkout (`clis/agent-config-sync/tests/flow/checkout.rs:1-3`) is a cheaper and more faithful double.
- The entry point takes its context (`cli::run_in`, `cli.rs:108`).

Tests:
- **Unit:** the core, with plain functions as doubles (`libraries/hook-runtime/tests/guard.test.ts:101-115` already does this).
- **Flow:** the entry function in process with fake adapters. Examples: `dispatch(payload, { run })`; the device-frame wiring with a fake location and a captured `ShellBridge`, driven as wf drives it (`clis/wf/src/browser_launch.rs:306-423`).
- **Integration:** one smoke per real entry point. Examples: `node src/dispatch.mjs` with a payload on fd 0; the built device-frame page in real Chromium (`clis/wf/tests/flow/browser.rs:1372-1386`).

Enforcement:
- TypeScript: a root `.oxlintrc.arch.json` run by each tool's `lint`, plus ls-lint whitelists per tool.
  - In `**/src/core/**`: `no-restricted-imports` for `node:*` except path, url and crypto, for the bare built-in names (`fs`, `fs/promises`, `child_process`, `net`, `http`, `https`, `os`, `worker_threads`), for `**/adapters/**`, `**/ui/**` and React; `no-restricted-globals` for process, window, document and location; `no-restricted-properties` for `globalThis.process`, `globalThis.window` and `globalThis.document`. A bare `fs` import and a `globalThis.process` read pass a `node:*`-only pattern, so both are planted in the dirty fixture.
  - In `**/src/ui/**`: no adapters.
- Rust: an ast-grep `acs-pure-modules` rule.
- Why not dependency-cruiser: hook-runtime is plain `.mjs` with no tsconfig, and the feature-kit factory hard-codes `tsconfig.json` (`depcruise.feature.cjs:35`).

### 5. Core library

Member: rca-core. wf-core (type 3) and every service core (type 1) follow the same purity rule.

Shape:
- Pure modules, port traits, and a `testing` module behind a feature.
- An adapter that several consumers share is its own adapter crate (type 6), not a feature of the core. A feature would not protect anyone: both consumers of rca-core's file store, wf (`dispatch_run.rs:8`, `dispatch.rs:6`) and rca-service (`application/runs.rs:11`), would enable it.
- Today `clis/rca-core/src/lib.rs:1-2` claims "Pure logic with no I/O", but `file_store.rs:19-54` calls `std::fs`. The 56-line file moves to a new adapter crate, `clis/rca-file-store`, used by wf's adapters and rca-service's infrastructure.
- `RunHost` (`run.rs:49`) is the port; `ScriptHost` (`tests/flow/script_host.rs:43`) is its fake.

Tests: unit tests, and in-process flow tests over a fake host.

Enforcement: `core-does-no-io` over `clis/rca-core/src`. No `compile_fail` doctest: rca-core depends only on serde, serde_json, sha2 and time (`clis/rca-core/Cargo.toml`), so a doctest that imports `cli_http` or `cdp` could never compile and proves nothing.

### 6. Adapter library

Members: cdp, cli-http, telemetry, `@lib/service-kit`, `@lib/browser-client`; `clis/rca-file-store` (new, Phase 5).

Shape:
- Flat modules, no layers and no product rules: value types and one I/O surface. The consumer defines its own port over it; wf's `Page` port wraps `cdp::Attached`.
- cli-http, used by at least 20 wf files, by cdp and by the listener, gets no `Transport` trait until a named flow test needs one. wf's use cases are tested over port fakes, and its real adapters against the loopback `tests/flow/support/fake_vm.rs`, so a scripted transport fake would have no consumer.

Tests: integration tests against a fake of the outside system, reported as integration:
- cdp: a fake Chromium on loopback;
- cli-http: tiny_http;
- browser-client: a fake `WF_BIN` plus a node:http server (`libraries/browser-client/tests/index.test.ts:8,24,69`).

Enforcement: lint only. The consumers' core rules forbid a core from importing an adapter library.

### 7. App shell

Member: `frontend/mla-tamagui` outside `features/`.

Shape:
- **`app/`:** expo-router.
  - `_layout.tsx` files compose providers, install the logger and the crash handler, and wire library ports to shell adapters.
  - Every other route file is a one-line re-export of a feature's `routes/*` module or of an `app-shell/` screen.
- **`app-shell/`:** the shell UI: drawer, debug FAB, log sheet, crash handler, showcase, charts spike.
- **`lib/`:** shell adapters for library ports (logging persistence and transports, dev restart) and platform shims. Only `app/` and `app-shell/` import it.
- **`features/`.**
- **No `components/` directory.** dashboards becomes a feature.

Dependency rule: the shell reaches a feature only through its barrel, `routes/*` or `testing`. Nothing under `features/` imports the shell.

Tests:
- Shell flow tests in `tests/flow` (drawer, debug sheet, theme). They import features only through those three entry points.
- Browser and Android e2e.

Enforcement: the dependency-cruiser `appShell` preset in mla-tamagui's `lint`, with rules `route-files-are-reexports`, `shell-sees-feature-surface-only` and `lib-is-shell-only`.

### 8. Hexagonal feature

Members: leetcode, tamil-writing, coding-agent, service-report, dashboards. A feature takes this shape when it calls a backend or persists data.

Rings:
- **`domain/`:** entities, zod schemas, pure rules, bundled catalogues, and `domain/ports.ts`, which holds `<Feature>Ports` plus the port request, response and error types.
  - It imports only `domain/`, zod, `*-contracts` packages, `@lib/voice-edit-dsl` and `@lib/logging`.
  - It never imports react, react-native, expo-*, zustand, `@lib/app-di`, `@lib/app-config` or the design system.
- **`data/`:** adapters.
  - Each is a factory `create<Tech><Port>(config, io?)` with an injectable I/O primitive (fetchImpl, WebSocketImpl, storage). `reportingGateway.ts:106-110` and coding-agent's `codingAgentConfig.ts:38` already work this way.
  - A fixture mode that ships in the product stays here: leetcode's `data/fixturePorts.ts`, which `di/fixtureOverrides.tsx:4-8` mounts behind `EXPO_PUBLIC_LEETCODE_FIXTURES` and `frontend/mla-tamagui/e2e/capture-leetcode-shots.mjs` relies on. `testing/` re-exports it; it never moves into `testing/`.
  - `data/` imports `domain/` only.
- **`state/`:** the application ring.
  - It holds zustand stores, the use-case hooks that call ports, and `state/ports.tsx` (the context plus `use<Feature>Ports()`, which throws outside its provider).
  - It imports `domain/` only.
- **`ui/`:** screens and components, importing `state/` and `domain/`.
  - A native view is UI.
  - A self-contained widget counts as `ui` (leetcode `code-editor/`).
  - A sub-feature repeats the ring names inside itself (leetcode `plan/ui`, `plan/state`).
- **`di/`:** the composition root and the only reader of `@lib/app-di`.
  - `<Feature>Container` builds the default adapters from `data/` with `@lib/app-config` values, merges `useDependencyOverrides<Partial<<Feature>Ports>>('<feature>')`, and provides the result through `state/ports.tsx`.
  - It also owns the native providers that adapters need (`ExpoAudioRecorderProvider`).
- **`routes/`:** the modules `app/` re-exports. Each mounts the `di/` container once, in `routes/_layout.tsx`, and imports `di/` and `ui/` only. A route whose screen reads `use<Feature>Ports()` sits under the one `app/` subtree whose `_layout.tsx` re-exports that layout. Today coding-agent's routes span `app/(app)/(content)/coding-agent/*` and `app/coding-agent/[threadId]/settings.tsx`, and neither tree has a coding-agent layout; C2 moves the settings route under a tree whose layout mounts the coding-agent container, keeping its `transparentModal` presentation (`app/_layout.tsx:124-126`), before C4 puts `ThreadSettingsSheet` on a port. leetcode's home route `app/(app)/(content)/index.tsx` sits outside `app/leetcode/`, and `ProblemListScreen` reads no port today. A shell flow test renders every feature route through `createFlowApp` and fails on a missing provider.
- **`testing/`:** `createFake<Feature>Ports()`, the harness-only doubles, a re-export of any shipped fixture adapter from `data/`, and e2e fake servers in `testing/node/`. Production rings never import it.
- **`index.ts`:** the barrel.

Dependency rule:
- `routes → di → data → domain`, `routes → ui → state → domain`, and `di → state` for the provider.
- The public surface is exactly the barrel, `routes/*` and `testing`.
- Features are leaves: no feature imports another feature or the shell.

Retired directory names: `contracts/`, `use-cases/` and `lib/` become `domain/`; `hooks/` and `integration/` become `state/`; `fixtures/` moves to `testing/`; `auth/` is deleted (dead code in leetcode only).

This replaces `ui → integration → state → data → domain` (root.md:21, `depcruise.feature.cjs:2`), which puts adapters below the application. The results are in the tree today:
- port interfaces in `data/` (coding-agent `data/orchestrationGateway.ts:40`);
- `ui → data` allowed by the shared preset (`depcruise.feature.cjs:7`);
- override reads in screens (coding-agent `ui/ThreadListScreen.tsx:102`, `ui/DispatchRunScreen.tsx:56`).

Tests:
- **Unit:** `domain/` with no doubles.
- **Adapter (flow):** the real factory with an injected transport, which is flow by `verification-ladder.md:44` (the dependency is replaced by passing a different object); a test that starts a loopback server instead is integration. Assert three things: the request it sends, the response mapping and the error mapping. One shared port-contract suite runs against a shipped fixture adapter and the live adapter, so the double cannot drift.
- **Flow (`test:flow`):** whole routes mounted through `createFlowApp`.
  - The only injection is `overrides: { '<feature>': Partial<<Feature>Ports> }`, which only `di/` reads. So every flow test boots the real composition root.
  - Doubles come from `testing/`, native modules from the harness stubs.
  - No `vi.mock` (banned by `frontend/.oxlintrc.json:126-134`) and no `vi.spyOn` on a project module (banned by 2.2).
  - Feature behaviour is tested in the feature. The flow glob collects only `{src,tests}/**` relative to the package (`frontend/libraries/flow-test-harness/src/vitest/flowTestConfig.ts:24,270`), so an in-app feature's flow tests run only after unit F0 widens mla-tamagui's flow include.
- **E2e:** browser and Android harnesses, with backends from the deployed services or `testing/node`.

Enforcement:
- The dependency-cruiser `hexagonalFeature` preset, every rule at `error`:
  - `every-module-sits-in-a-ring`
  - `domain-is-pure`
  - `data-implements-domain-ports`
  - `state-uses-ports`
  - `ui-uses-state`
  - `only-di-composes`
  - `routes-mount-di-and-ui`
  - `feature-is-a-leaf`
  - `no-circular`
- oxlint:
  - `no-restricted-globals` for fetch, WebSocket, XMLHttpRequest and EventSource outside `data/` and `lib/`: 0 hits today.
  - `@react-native-async-storage/async-storage` imported only in `data/` and `state/`: 1 hit today (`coding-agent/src/ui/ThreadSettingsSheet.tsx:12`).
- Draft-rule hits from the app assessment (unreproduced: the draft rules were not committed): leetcode 7 under the final rule, tamil-writing 4, coding-agent 21, service-report 3, dashboards 5. 2.4 measures the real counts when it writes the baselines.

### 9. Presentation feature

Members: settings, and dev-tools if it is kept (see [Open questions](#open-questions)).

Shape:
- `ui/`, `routes/`, and an optional `state/` for local UI state. No domain, data, di or ports.
- Device services are called directly. The harness has no stub for them; in a flow test they run through react-native-web or the module's own web build.
- The feature becomes type 8 with its first backend call, persisted store or business rule.

Tests: flow tests.

Enforcement: the `presentationFeature` preset, with `every-module-sits-in-a-ring` (ui, state, routes, testing), `feature-is-a-leaf` and `no-circular`.

### 10. Contracts package

Members: `@app/remote-chat-agent-contracts`; also `@lib/leetcode-contracts` until it is folded into leetcode, and `@cli/harness-contracts` until browser-harness collapses.

Today none fits the shape fully. `@cli/harness-contracts` has no dependencies at all and holds a device registry and geometry functions (`tooling/browser-harness/contracts/src/index.ts:209-444`, for example `getDeviceSpecById` and `resolveOrientedSafeArea`). `@lib/leetcode-contracts` is imported only by leetcode, and `@app/remote-chat-agent-contracts` only by coding-agent.

Shape:
- A flat `src/` of zod schemas and inferred types. zod is the only dependency. No React, React Native, node built-ins or I/O.
- Exports point at `src`.
- It exists only while two or more packages consume it; otherwise its schemas belong in the consumer's `domain/`.
- A wire contract that a Rust service mirrors carries golden JSON fixtures. zod parses them in vitest and serde in `cargo test`.

Tests: unit tests that parse the goldens.

Enforcement: the `contractsPackage` preset: a module imports only its own files and zod.

### 11. Pure library

Members: `@lib/gesture-geometry`, `@lib/voice-edit-dsl`, `@lib/eslint-plugin-mobile-learn`, `@lib/app-di` (with react allowed), and `@lib/app-config` once it is a pure `resolveAppConfig({ env, launchArgs, devFile, allowedRoutes })`. Its env is then one object of literal `process.env.EXPO_PUBLIC_X` reads in the app's composition root, because Expo inlines only static member reads (choice 7). That is a parameter, not a port.

Shape:
- A flat `src/` with a barrel, and an optional `contracts/` folder for zod.
- No ports, adapters or DI.
- It imports only relative modules and an allow-list of pure dependencies (zod, estree types, react for app-di). No `node:` built-ins, react-native, expo-*, globals or workspace packages.

Tests: unit tests only, table-driven.

Enforcement: the `pure` library preset (no node core, no workspace package, `no-circular`), plus the oxlint globals override.

### 12. Hexagonal library

Members: `@lib/logging`, `@lib/rn-agentation`.

Rings, as folders of one package:
- **Core:** domain types, rules and application functions or hooks. Each takes its ports as parameters, through a `createX(ports)` factory or provider props.
- **Ports:** interfaces declared in the core and named after the need: LogPersistence, Clock, DocumentTreeDriver, KeyValueStorage, and AnnotationSink only if open question 8 keeps the HTTP sink. A device service with one call site gets no port (choice 5): rn-agentation's clipboard is one `Clipboard.setStringAsync` call (`src/headless/useAgentationState.ts:6,302`) that no test drives.
- **Adapters:** one file per technology. They are the only files that import expo-* or AsyncStorage, or touch fetch, `window` or `process.env`. `.web` and `.native` platform forks live only here.
- **Composition:** the barrel or a defaults module binds default adapters. The host app is the real composition root: `frontend/mla-tamagui/lib/logger.ts`, `app/_layout.tsx` and the features' `di/`.
- **Testing:** `src/testing/`, exported as `./testing`.

Existing folder names are mapped to roles in each library's `.dependency-cruiser.cjs`, as `withDomainDirs` does today. A folder is renamed only when it mixes roles.

Dependency rule:
- adapters → core; core → nothing outward; composition → core and adapters; consumers → the barrel.
- Subpath exports are allowed only for bundle size, build-time entry points (the logging babel plugin) or `./testing`.
- The static `Logger` is the documented ambient exception (root.md: `Logger.*`, call site injected at compile time).
- Between packages, a hexagonal library may depend on `@lib/logging` only.

Tests:
- **Core:** unit tests with the library's own doubles. Never `vi.mock` the library's own modules (banned by 2.2); there are 9 such sites in app-config today.
- **Adapters:** flow tests by `verification-ladder.md:44`, because jsdom, a tmp dir and the harness AsyncStorage stub are passed in; integration only for an adapter tested against a loopback server or a stub binary.

Enforcement:
- The `hexagonal` library preset:
  - `core-imports-no-io`: node core, plus a node_modules regex for expo-*, AsyncStorage, react-native-webview and react-native-mmkv that works through the `.pnpm` paths;
  - `only-composition-imports-adapters`;
  - `no-circular`;
  - `no-app-imports`.
- The oxlint globals override.

### 13. Component library

Member: `@lib/tamagui-design-system`.

Shape:
- The order is `theme/` → `primitives.tsx` and `icons.ts` → `ui/` → `composites/`. `primitives.tsx` is the single door to tamagui and `icons.ts` the single door to lucide (`frontend/.oxlintrc.json:62-71`; the package exports `./primitives` and `./icons` separately). `lib/` is a leaf holding UI-state stores and platform sources.
- It is not hexagonal: the components are the delivery.
- Its two I/O touches are chosen by `.web` / `.native` file forks, with no port:
  - key-value storage: `lib/keyValueStore.ts` (localStorage or memory) plus a new `lib/keyValueStore.native.ts` over `@react-native-async-storage/async-storage`, a peer dependency as in rn-agentation. A setter injected by the host cannot work: `theme/state/fontPresetStore.ts:18-33` and `presetThemeStore.ts:17-41` call `create()(persist(...))` at module scope with no `skipHydration`, so they hydrate when `app/_layout.tsx` imports the design system, before its body runs;
  - the keyboard source.
- Deep subpath exports are deliberate, because Metro does not tree-shake.

Tests:
- **Unit:** theme, token and chart math.
- **Flow:** through the app's flow tests. The design system must not dev-depend on flow-test-harness: the harness already depends on it (`render.tsx:6-7`), so the reverse would be a cycle.

Enforcement: the `component` preset (direction plus `no-circular`). The assessment measured a draft of it at 84 modules and 0 violations (unreproduced: the draft was not committed).

### 14. Test harness

Members: `@lib/flow-test-harness`. The `e2e/*.lua` files are test cases, not packages; wf's device harness runs them.

Shape:
- A driver API: `flowApp`, `render`, `route`, `interactions`, `router`, `storage`.
- Test adapters that replace third-party externals at the native-module boundary (`src/stubs`).
- The host wiring as composition root: the vitest config, reporter and setup.
- A pure report model.
- It stays feature-agnostic: it never ships a feature's port doubles and never imports app or feature code.

Enforcement: the `harness` preset. Runtime files may not import `src/vitest/`, except the `warningGate` re-export; nothing may import `frontend/mla-tamagui`; `stubs/` is excluded.

### 15. Glue

Members:
- `scripts/postinstall.mjs` and `scripts/prepare.mjs`;
- `tooling/nx`, `clis/cm6`;
- the container scripts in `infra/oracle-cloud/{agent-shell,metro-runner,build,scripts}`;
- `.opencode/plugin/json-hooks-adapter.js` and `.agents/hooks/session-*.sh`;
- the budget-guard function and the rca stt sidecar.

Shape:
- Flat, one file per entry point.
- Glue holds no decision. When one appears (a retry policy, a classification, a selection, parsing), it moves into wf (root.md:27) or into a type-4 package.
- Two pieces are already over the line:
  - `infra/oracle-cloud/scripts/retry-apply.sh`: 325 lines of region and shape logic, with a raw `terraform apply -auto-approve` at `:276`;
  - provision-apps.js, embedded in `infra/oracle-cloud/cloud-init.sh.tftpl:131-467`.

Tests: the program it drives.
- `tooling/nx` is covered by `clis/wf/tests/flow/nx_cache.rs`.
- Container glue is covered by the deploy legs.
- Bootstrap scripts are covered by `pnpm install`.

Enforcement: an ls-lint whitelist, so `scripts/` holds only `postinstall.mjs` and `prepare.mjs`.

### 16. Config and data

Members:
- the Terraform root, compose files, Dockerfiles, `vm-apps.json`, `infra/tailscale/policy.hujson`;
- `nx.json` and the lint, ls-lint and ast-grep configs;
- `frontend/feature-kit`;
- `clis/secrets/src/data/registry.json`, `clis/app-bench`;
- the `@infra/watchdog` shim, the vnc-auth-capture image files, `pagerduty-transform.js`.

Shape: no architecture. Format and schema checks guard them, plus static config-contract tests that some package owns. `infra/oracle-cloud/tests/compose-healthchecks.test.mjs` is the model.

Enforcement: a new `@infra/oracle-cloud` config package.
- `lint` = `terraform fmt -check -recursive`.
- `test:unit` = the node:test config tests.
- Neither check runs today. The assessment found the healthcheck test red (gh-runner at `agent-shell/docker-compose.prod.yml:243` has no healthcheck) and `terraform fmt -check` failing on `autonomous-db.tf`.

## Inventory

One row per package. A Rust service's core and rs crates share a row. Effort: none, S, M, L or XL.

| Package | Path | Type | Target shape | Today | Effort |
|---|---|---|---|---|---|
| workspace root (scripts) | `scripts/` | 15 | only `postinstall.mjs`, `prepare.mjs` | guards, hooks and a Stop check live here; the 56 guard tests run nowhere (no root test script) | M |
| mla-tamagui | `frontend/mla-tamagui` | 7 | `app/` re-exports, `app-shell/`, `lib/` adapters | `showcase.tsx` (298 lines) sits in `app/`; dashboards sits in `components/`; `lint` is oxlint only | M |
| feature-leetcode | `frontend/mla-tamagui/features/leetcode` | 8 | rings, plus the `plan/` sub-feature and the `code-editor/` widget | ports and container exist (`contracts/ports.ts:41-143`, `di/LeetCodeContainer.tsx:12-24`); no `domain/`; ui and routes import `data/`; its rules run by hand only | M |
| feature-tamil-writing | `frontend/mla-tamagui/features/tamil-writing` | 8 | add ports, `di/`, `testing/` | pure `domain/` (4,159 lines); 4 screens import `data/`; nothing cruises it | M |
| dashboards | `frontend/mla-tamagui/components/dashboards` → `features/dashboards` | 8 | an in-app feature | port exists (`data/reportingGateway.ts:42`); `DashboardsIndexScreen.tsx:91` builds the HTTP gateway itself | M |
| @feature/coding-agent | `frontend/mla-tamagui/features/coding-agent` | 8 | ports in `domain/ports.ts`, one container | ports live in `data/`; 21 draft-rule hits; fixtures ship in production rings | L |
| @feature/service-report | `frontend/mla-tamagui/features/service-report` | 8 | ports plus a container | a per-function override (`state/useHealthReportHistory.ts:32-35`); the adapters read env | M |
| @feature/settings | `frontend/mla-tamagui/features/settings` | 9 | `ui/`, `routes/`, a leaf | ui only; exports `backTo`, which dashboards imports | S |
| @feature/dev-tools | `frontend/mla-tamagui/features/dev-tools` | dissolve into `app-shell/` (or 9) | — | its DI is dead (`src/di/runtime.ts`); its overlay renders null | S |
| @lib/leetcode-contracts | `frontend/mla-tamagui/features/leetcode-contracts` | fold into leetcode `domain/wire/` (10 if kept) | — | one consumer | S |
| @app/remote-chat-agent-contracts | `backend/remote-chat-agent-contracts` | 10 | `src` exports, golden fixtures | pure; no `lint`; `.` needs a dist build | M |
| feature-kit | `frontend/feature-kit` | 16; becomes `@lib/feature-kit`, the home of the TS rules | presets plus planted-violation tests | a layer factory for 4 packages; no package.json | S |
| @lib/app-config | `frontend/libraries/app-config` | 11 after open question 7 | a pure `resolveAppConfig`; the literal env object is read in the app | reads env at import; 9 self-mock sites; no `lint` | M |
| @lib/app-di | `frontend/libraries/app-di` | 11 | typed `OverrideKey<T>` | untyped string keys; tracked `dist/`; no `lint` | S |
| @lib/flow-test-harness | `frontend/libraries/flow-test-harness` | 14 | feature-agnostic | ships leetcode doubles (`src/doubles/`) | M |
| @lib/gesture-geometry | `frontend/libraries/gesture-geometry` | 11 | unchanged | pure; no `lint`; tracked `dist/` | S |
| @lib/logging | `frontend/libraries/logging` | 12 | plus `./testing` | ports exist; one layer edge (`primitives/secretRedaction.ts:11`); no log capture for tests | M |
| @lib/registry-service | `frontend/libraries/registry-service` | delete | — | no importer; wf owns the registry | S |
| @lib/rn-agentation | `frontend/libraries/rn-agentation` | 12 | a KeyValueStorage port, and AnnotationSink only if open question 8 keeps the sink | the React layer reaches adapters (5 edges, unreproduced); 8 oxlint errors; no `lint` | M |
| @lib/tamagui-design-system | `frontend/libraries/tamagui-design-system` | 13 | a `.native` key-value store over AsyncStorage | a theme is lost on native restart (`lib/keyValueStore.ts:11,19-21`) | M |
| @lib/voice-edit-dsl | `frontend/libraries/voice-edit-dsl` | 11 | unchanged | pure; 1 oxlint error; no `lint`; tracked `dist/` | S |
| @lib/browser-client | `libraries/browser-client` | 6 | unchanged, the reference adapter-only shape | fits | none |
| @lib/eslint-plugin-mobile-learn | `libraries/eslint-plugin-mobile-learn` | 11 | unchanged | a type-only cycle (`src/rules/index.ts:1-2`); no `lint` | S |
| @lib/hook-runtime | `libraries/hook-runtime` | 4 | `src/core`, `src/adapters`, `src/steps` | I/O inside `guard.mjs` (`:1-4, 245-249, 273-301, 337-345`); a missing condition script fails open; no `lint` | M |
| @tooling/browser-harness, @cli/harness-{contracts,data,di,use-cases,ui} | `tooling/browser-harness` | 4 (one package) | `src/core`, `src/adapters`, `src/ui`, `src/main.tsx` | 6 packages for 1,390 lines; the walls do not hold under hoisting; no tests; not an input of any test | M |
| @tooling/wf-blackbox | `tooling/wf-blackbox` | delete | — | broken since 7dbaa9b63 (12 Oct): its one test reads a deleted file | S |
| tooling/nx | `tooling/nx` | 15 | unchanged | `HARNESS` lists wf files by hand (`e2e-tests.js:10-29`) and omits the device-frame page | S |
| clis/cm6 | `clis/cm6` | 15 | unchanged | one esbuild script | none |
| @app/leetcode-backend (leetcode-backend-core, leetcode-backend-rs) | `backend/leetcode-backend` | 1, three rings | the reference | clean; `arch:services` not in its `lint` | S |
| @app/leetcode-auth-cookie-capture (core, rs) | `backend/leetcode-auth-cookie-capture` | 1, four rings | the reference | an adapter reads `WF_BIN` (`rs/src/infrastructure/browser_daemon.rs:33`); no `lint` | S |
| @backend/reporting-service (core, rs) | `backend/reporting-service` | 1 | `Ports` in the core | `Ports` sits in `api/wiring.rs:10-18`; `GIT_COMMIT` is read in `main.rs:45`; no `lint` | S |
| @app/remote-chat-agent (remote-chat-agent-core, rca-service) | `backend/remote-chat-agent` | 1 | no ignores | 12 api ignores plus the application glob; env reads in every ring of the `rs` crate (none in the core); a second composition root (`api/app.rs:110-127`); the core reads the clock and UUIDs (`core/src/domain/ids.rs:8,15,19`) | XL |
| @infra/watchdog (watchdog-core, watchdog-rs) | `infra/oracle-cloud/watchdog`, `backend/watchdog` | 1, no `api/` | unchanged | clean; the package and its crates live in different trees | S |
| @lib/service-kit | `backend/service-kit` | 6 | unchanged | fits; no arch check covers it | none |
| @backend/vm-ops-api (vm-ops-api-core, vm-ops-api-rs) | `backend/vm-ops-api` | 1 | the Rust port of the Node service, written in the type-1 shape (`docs/plans/vm-ops-api-rs/plan.md`) | new with the port: `pnpm run arch:services` covers it with no ratchet; no `lint` script | none |
| vm-ops-api's compose project (no package since S4) | `infra/oracle-cloud/vm-ops-api` | 16 | compose, `.env.defaults`, `deploy.sh`; the dashboard page and the integration suite moved into `backend/vm-ops-api` | unchecked | none |
| @cli/vnc-auth-capture (image and vnc-auth-listener) | `clis/vnc-auth-capture` | 16 (image), 1 (core and listener crates) | `core` (session engine, ports, `testing`) and `listener` (axum, adapters, main) | the listener compiles wf sources by `#[path]` (`listener/src/main.rs:4-16`) | M |
| wf | `clis/wf` | 3 | `wf` (cli, adapters, world) over `wf-core` | 87 flat top-level files; 96 `Command::new` sites in 48 files; 2 port traits; `test = false` | XL |
| wf-core (new) | `clis/wf-core` | 3 (its core crate) | — | does not exist | S, plus the family moves |
| agent-config-sync | `clis/agent-config-sync` | 4 (Rust) | pure parse split from load | `contract.rs` reads files while it validates; source dir is `src-rs` | S |
| rca-core | `clis/rca-core` | 5 | no I/O; the file store moves to `clis/rca-file-store` | `file_store.rs` does fs despite `lib.rs:1-2` | S |
| cdp | `clis/cdp` | 6 | unchanged | fits; its suite is integration | none |
| rca-file-store (new) | `clis/rca-file-store` | 6 | `file_store.rs` (56 lines) from rca-core | — | S |
| cli-http | `clis/http` | 6 | unchanged; a `Transport` trait only when a named flow test needs one | no seam for callers | none |
| telemetry | `clis/telemetry` | 6 | unchanged | its inline tests run only in the post-land `rust-clis` hook (`lefthook.yml:55-58`), never in CI (no `tests/flow.rs`) | S |
| Cargo workspaces | `backend/Cargo.toml`, `clis/Cargo.toml` (Nx `cargo-backend`, `cargo-clis`, `tooling/nx/cargo-crates.js:88`) | 16 | the workspace stops enabling tokio `fs`, `net` and `process` (`backend/Cargo.toml:31`); each `rs` or adapter crate opts in | the workspace features reach the remote-chat-agent, reporting-service and watchdog cores | S |
| e2e-* (9 Nx projects from `tooling/nx/e2e-tests.js`) | `e2e/*.lua` | none: test cases, run by wf's device harness | unchanged | — | none |
| clis/secrets | `clis/secrets` | 16 | unchanged | — | none |
| clis/app-bench | `clis/app-bench` | 16 | unchanged | — | none |
| @infra/oracle-cloud (new) | `infra/oracle-cloud` (`*.tf`, cloud-init, `scripts/`, `tests/`) | 16 and 15 | a config package with `lint` and `test:unit` | the config tests are red and unrun | M |
| provision-apps (to extract) | `infra/oracle-cloud/provision/` | 4 | core plan, adapters for bws, docker, https | 337 lines of Node inside `cloud-init.sh.tftpl:131-467` | M |
| agent-shell, metro-runner, build, logger-cli, agent-base image, tailscale policy | `infra/oracle-cloud/*`, `infra/docker`, `infra/tailscale` | 15 and 16 | unchanged | `agent-shell/tests/*.test.mjs` run nowhere | S |
| budget-guard, pagerduty transform | `infra/oracle-cloud/functions/budget-guard`, `pagerduty-transform.js` | 15 | unchanged | the spend decision has no test | none |
| rca stt sidecar | `backend/remote-chat-agent/stt` | 15 | unchanged | — | none |
| agent hook shims and bootstrap | `.opencode/plugin`, `.agents/hooks`, `.agents/setup.sh`, `.conductor/` | 15 | `rtk-rewrite.sh` moves into hook-runtime | its tests already live in hook-runtime (`tests/rtk-rewrite.test.ts:8`) | S |

## Migration

Each unit is one commit. Each commit goes through lefthook, lands on `dev` (`git fetch origin dev && git rebase origin/dev`) and leaves every gate green. A unit that moves code does not change behaviour in the same commit. A unit that fixes debt deletes its own baseline or `ignores` entries in that commit. Mechanical units (moves, renames, import rewrites) go to `wf offload ask --batch` per `.claude/rules/delegation.md`; rule design and review stay with Claude.

The order: docs first, so every later unit cites the doc; gates and rules next, because they are cheap and make every later unit measurable; then the packages, highest value first.
- The reference feature leetcode does not meet its own rule, and the two gap docs record that.
- rca holds almost all of the services' debt.
- The libraries and tooling come after them.
- wf comes last: it is XL, and its black-box suite already guards it.

### Phase 0. The doc cleanup and this spec

Packages: none (docs and instructions only).

#### The doc cleanup

The archived "Clean architecture standard" is replaced, not revived. It is `docs/archive/solutions/architecture/clean-architecture.md` (208 lines) plus its 43-line summary `clean-architecture-standard.md`.
1. **One new doc**, `docs/solutions/architecture/hexagonal-architecture-by-package-type.md`, written beside this spec on 21 Oct. root.md says long-form rules live in `docs/solutions/`; before this doc, `docs/solutions/architecture/` held only `patched-dependencies.md`. The doc holds:
   - the rule, the vocabulary and the shapes;
   - each package type today and its target;
   - every rule, with where it runs;
   - which test covers which ring;
   - what it replaces.
2. **What survives from the archived doc:** TypeScript adapters are factory functions, and the composition root builds the graph while its entry point runs it (`di/LeetCodeContainer.tsx`, `routes/_layout.tsx`).
3. **What it retires**, one line each in its "Retired" section:
   - the five roles and "contracts, not domain";
   - two-tier DI (built in the TypeScript packages, removed with them);
   - Nx module boundaries (enforced until `923d53408`);
   - package-per-layer;
   - the feature triad;
   - convention-lint's rule names;
   - the greenfield wf;
   - the five package categories of the coverage-debt goal;
   - the rest of `docs/archive/solutions/architecture/`, kept as history.
4. **The archived pair stays in `docs/archive/` as history**, and the new doc names it as replaced. No live file may link it. Two still do at HEAD: `frontend/mla-tamagui/.dependency-cruiser.cjs:7` cites its old path, and `docs/gaps/feature-di-migration.md:13-14` links it and a sibling.

#### Units

- 0.1 **The doc and this spec.**
- 0.2 **Instructions.**
  - `.agents/instructions/root.md:19-21`: the rule and a link to the doc. Scope the route rule to route files under a feature prefix, and keep the leetcode check named (`pnpm --filter mla-tamagui run arch`) until 2.4 folds it into the app's `lint`.
  - `.agents/instructions/frontend/mla-tamagui/features.md`: types 8 to 10; dependency-cruiser runs only in the four `@feature/*` packages.
  - `features.md:32` and `.agents/instructions/frontend/mla-tamagui.md:9`: a new port fake ships from the port owner's `testing` entry; only a native package is a harness stub. Link the doc.
  - `frontend/libraries/flow-test-harness/README.md` "A double" (:190-210): doubles live with the port owner; the harness holds only stubs and its driver API; `src/doubles/` is debt that Phase 5 moves.
  - Then run `pnpm run sync:agent-config`.
- 0.3 **Docs that contradict the rule:**
  - `CONCEPTS.md`: retire Feature Triad; add Port, Adapter and Composition Root, worded as definitions, not as the state of the tree; merge `Flow test` (`:91`) and `Flow Test` (`:141`), keeping the harness's native-module stubs; link the doc from Ring.
  - `verification-ladder.md:58-59`, with the same native-stub clause.
  - `flow-tests-mount-whole-routes.md:21,50,53,59,64,66`.
- 0.4 **The rest.**
  - The `.dependency-cruiser.cjs:7` comment cites the new doc.
  - `frontend/feature-kit/depcruise.feature.cjs:1-2`: the header says the preset is today's chain, not the target, and links the doc (type 8).
  - `standard-crates.md`: the status names all five services and links the doc; `:101` points at Phases 4 and 6.
  - `greenfield/requirements.md:269`, `ts-removal/plan.md:39` and `docs/overlays-as-routes.md:268-269`.
  - `greenfield/clean-code-audit.md`, `contract.md` and `p6-workflow-cli.md`: one status line each, because they describe the layering of the deleted `clis/workflow-cli` as current.
  - `.github/workflows/architecture.yml:3`: the header says the workflow runs by hand.
  - `docs/gemini-context/system-prompt.txt`: the architecture rules match the doc.
- 0.5 **Gaps and goals.**
  - `docs/gaps/feature-di-migration.md`: its target becomes Phase 3; fix its frontmatter paths and links.
  - `docs/gaps/leetcode-ui-direct-io-debt.md`: its verify becomes ``! git grep -n "from '../data/" -- frontend/mla-tamagui/features/leetcode/ui``, which exits 1 while the debt remains.
  - `docs/plans/rca-hexagonal/plan.md`: link the doc (type 1) and Phase 4 R6; its "Done when" names the ast-grep ignores only, so it says the other rca debts (core clock and ids, the second composition root, env reads) are tracked in Phase 4.
  - Goals that rest on retired architecture: `2026-08-22-coverage-debt-and-browser-consolidation.md` and the queued `2026-08-27-metro-memory-footprint.md` and `2026-08-27-tamagui-second-app.md` become `status: superseded` with a line linking the doc; the completed `2026-08-22-test-and-architecture-reset.md` gets the same line.

Units 0.1 to 0.5 were drafted on 21 Oct beside this file and were uncommitted when it was written; at that point the `standard-crates.md` status (0.4) still named three services. They land together, in one commit. The `terraform fmt` change to `infra/oracle-cloud/autonomous-db.tf` in the same working tree belongs to 1.5 and stays out of that commit.

Done when (after `git add` of the doc and `docs/plans/hexagonal-architecture/`: `wf check --dead-refs` reads `git ls-files` (`clis/wf/src/dead_refs.rs:40-41`), so every new pointer to an untracked file counts as dead):
```sh
pnpm run sync:check && wf check --dead-refs && wf check --budget \
  && git ls-files --error-unmatch docs/solutions/architecture/hexagonal-architecture-by-package-type.md \
  && ! git grep -nF 'ui → integration → state → data → domain' -- . ':!docs/archive' ':!docs/audits' ':!docs/plans' \
  && ! git grep -nE '(docs/|^[[:space:]]+- )solutions/architecture/(clean-architecture|two-tier-di-containers)' \
       -- docs/gaps docs/solutions frontend .agents CONCEPTS.md \
  && ! git grep -nE '\]\([^)]*archive/solutions/architecture/(clean-architecture|two-tier)' -- . ':!docs/archive'
```
At HEAD the second grep finds `.agents/instructions/root.md` (rendered into `AGENTS.md` and `CLAUDE.md`) and `frontend/feature-kit/depcruise.feature.cjs:2`, and the third finds three stale links (`feature-di-migration.md:13-14` and `.dependency-cruiser.cjs:7`). The last finds none; it guards against a link to the archived pair under its archive path, which the third cannot see.

Risks:
- **root.md counts against `wf check --budget`.** The paragraph must not grow; the detail goes to the doc.
- **Two docs can drift.** The doc and this spec state the same rule. A unit that changes a rule or a check edits both in the same commit.

### Phase 1. Every package reaches a gate

Packages: every package; dead ones are deleted.

Units:
- 1.1 **Pre-commit lint reaches every package.**
  - `wf check --staged` takes lint owners from `workspace_packages()` (`clis/wf/src/verify.rs:188-221`) instead of `WORKSPACE_PARENTS` (`:285`).
  - Flow test in `clis/wf/tests/flow/check.rs`. Given a staged `frontend/libraries/x/src/y.ts` and a staged `frontend/mla-tamagui/features/settings/src/ui/z.tsx`, when `wf check --staged` runs, then `pnpm run --if-present lint` runs in `frontend/libraries/x` and in `features/settings`.
- 1.2 **Delete dead code**, one unit each, so no rule is written for it:
  - `@lib/registry-service`, with its dependency at `frontend/mla-tamagui/package.json:65` and its tracked `dist/`; repoint the fixture at `clis/wf/tests/flow/nx_cache.rs:241`.
  - `@tooling/wf-blackbox`.
  - leetcode `auth/LeetCodeBridge.tsx`; repoint `nx_cache.rs:228`.
  - dev-tools `src/di/`, the `configureDevToolsRuntime` call (`app/_layout.tsx:44-48`), and the unread `'dev-tools'` overrides in `tests/flow/debugLogSheet.flow.test.tsx` and `rootTheme.flow.test.tsx`.
  - `scripts/build-and-push-image.sh`.
- 1.3 **A `lint` script for each project without one**, after fixing that project's current oxlint errors: app-di 1, rn-agentation 8, voice-edit-dsl 1 (counts from the libraries assessment, unreproduced). Rust service packages get `pnpm -w run arch:services`. That `lint` runs in CI, and at pre-commit only when a JavaScript or TypeScript file of the package is staged (`clis/wf/src/verify.rs:167-169,364-365`); a staged `.rs` file reaches the rules through the `service-architecture` job.
- 1.4 **Rust crates in CI.**
  - Add crate `unit` and crate `lint` to `test_plan.rs`. `unit` is two commands, because cargo refuses to mix `--doc` with other target options ("can't mix --doc with other target selecting options", cargo 1.97): `cargo test --locked -p <crate> --lib --bins`, then `cargo test --locked -p <crate> --doc` for a crate with a lib target. `lint` is `cargo clippy --locked -p <crate> --all-targets -- -D warnings`, plus `ast-grep scan --config clis/sgconfig.yml` once it exists.
  - Add the cached Nx targets `cargo-unit` (both commands) and `cargo-lint` beside `cargo-flow` (`tooling/nx/cargo-crates.js:119-140`).
  - Drop `test = false` (`clis/wf/Cargo.toml:11`). This starts running wf's 20 files with inline test modules (`git grep -lF '#[cfg(test)]' -- clis/wf/src`, 21 Oct). wf has no lib target until W1, so it has no doctests until then.
  - Fix the 9 clippy findings of 21 Oct (see Risks).
  - The post-land `rust-clis` glob (`lefthook.yml:56`) gains every member crate: `clis/{rca-core,cdp,http,telemetry}/**`, and `clis/wf-core/**` and `clis/rca-file-store/**` in the units that create them.
- 1.5 **The `@infra/oracle-cloud` config package.** Add `infra/oracle-cloud` to `pnpm-workspace.yaml` packages; without it every `pnpm --filter @infra/oracle-cloud` matches nothing and exits 0. Add gh-runner's healthcheck, or a named exemption with its reason in the test. Run `terraform fmt`. Add `lint` and `test:unit`, the latter covering `agent-shell/tests`.
- 1.6 **Guard tests run.**
  - First, `ruleProblems` in `guard.mjs:25-41` rejects a condition script that does not exist (fail closed).
  - Then move `scripts/guards/*` and `scripts/hooks/mask-large-output.mjs`, with their tests, into `libraries/hook-runtime/src/steps/`. In the same commit, update `deny.json`, `dispatch.mjs:24`, `lefthook.yml:33,38`, `clis/wf/tests/flow/secrets.rs:609,617` and `vitest.config.ts:15`.

Every filtered gate in this plan is written `pnpm --fail-if-no-match --filter <pkg> run <script>`: pnpm 11.5.2 exits 0 when a filter matches no package, and 1 with `--fail-if-no-match`.

Done when:
```sh
cargo test --manifest-path clis/Cargo.toml -p wf --test flow check:: \
  && pnpm ls -r --json | jq -e 'map(.name) | index("@infra/oracle-cloud")' >/dev/null \
  && pnpm ls -r --depth -1 --json | jq -r '.[].path' \
     | while read -r p; do jq -e '.scripts.lint' "$p/package.json" >/dev/null || exit 1; done \
  && pnpm --fail-if-no-match --filter @infra/oracle-cloud run lint \
  && pnpm --fail-if-no-match --filter @infra/oracle-cloud run test:unit \
  && pnpm --fail-if-no-match --filter @lib/hook-runtime run test:unit \
  && cargo clippy --manifest-path clis/Cargo.toml --workspace --all-targets -- -D warnings
```

Risks:
- **Latent errors.** New `lint` scripts surface errors nobody has seen; each lands only after its fixes.
- **clippy.** It ran clean with `-D warnings` in `backend/` and `clis/` on head `846130f` (9 Oct, `standard-crates.md:97`). On 21 Oct `cargo clippy --manifest-path clis/Cargo.toml --workspace --all-targets -- -D warnings` exits 101 with 9 findings, all in wf: four redundant closures (`src/run_report.rs:124,155,158,159`), one unnecessary closure with `bool::then` (`src/device/commands.rs:751`), and four very complex types in the flow tests (`tests/flow/browser.rs:81`, `report.rs:17`, `vm.rs:356`, `worker.rs:90`). The other five crates pass. 1.4 fixes the nine and keeps `-D warnings` (open question 11).
- **A moved guard condition fails open.** `guard.mjs:245-249` spawns node, gets exit 1 and skips the rule. That is why 1.6 lands the fail-closed check first.

### Phase 2. The rules, each with a baseline and a planted violation

Packages: feature-kit, mla-tamagui, every feature and frontend library, the Rust services, vm-ops-api, the CLI crates, and the type-4 tools.

Units:
- 2.1 **TypeScript presets.**
  - `frontend/feature-kit/depcruise.feature.cjs` gains `hexagonalFeature`, `presentationFeature`, `contractsPackage`, `appShell`, and the library presets `pure`, `hexagonal`, `component` and `harness`, each with `no-circular` at `error`.
  - `featureDepcruiseConfig` keeps its output until 2.4 switches each package.
  - feature-kit gets a package.json (`@lib/feature-kit`) with `test:unit`, and `frontend/feature-kit` joins `pnpm-workspace.yaml` packages. Without that entry `pnpm --filter @lib/feature-kit` matches nothing and exits 0, and `wf check --staged`, which finds packages through `pnpm ls -r` (`clis/wf/src/verify.rs:188-221`), never sees it.
  - feature-kit holds planted fixtures under `fixtures/planted/<preset>/{clean,dirty}/src`, one offending import per rule (see [Verification](#verification)).
  - `nx.json` gains a named input `archRules`, added to the `lint` target defaults (`nx.json:51-57`) and to `cargo-lint`: `frontend/feature-kit/**`, `.oxlintrc.arch.json`, `.ls-lint.yml`, both `sgconfig.yml` files, `clis/sgconfig.yml`, every `arch-rules/` and `arch-tests/` dir, and `clis/rule-tests/**`. Today only `frontend/feature-kit/depcruise.feature.cjs` is a shared input (`nx.json:12`), so a change to a rule or a rule test replays a cached `lint`. A flow test in `clis/wf/tests/flow/nx_cache.rs`: touching an arch rule makes the service `lint` a cache miss.
- 2.2 **oxlint overrides.**
  - In `frontend/.oxlintrc.json`: network globals outside `data/` and `lib/`, AsyncStorage outside `data/` and `state/`, and core globals for the pure and hexagonal libraries.
  - `no-restricted-properties` for `vi.spyOn` in `**/*.flow.test.{ts,tsx}`, beside the `vi.mock` ban (`frontend/.oxlintrc.json:126-134`). The two spies on non-project objects (`console` at `tests/flow/tamilWriting.flow.test.tsx:286`, React Native `Keyboard` at `tests/flow/sheetBranches.flow.test.tsx:27`) carry an `oxlint-disable-next-line` with the reason. The two spies on project modules (`settings.flow.test.tsx:54`, `tamilWriting.flow.test.tsx:247-248`) carry one too, until the settings unit and T3 delete them.
  - `vi.mock` and `vi.doMock` banned in `frontend/libraries/**` tests; app-config's 9 sites carry a disable comment until its Phase 5 unit deletes them.
  - A root `.oxlintrc.arch.json` for the type-4 tools (bare built-in names and `globalThis` reads included, see type 4).
  - The planted cases join the 2.1 suite.
- 2.3 **Mechanical renames first**, one `git mv` unit per feature, before any baseline is written, so no baseline is keyed by a retired directory name. Each keeps today's preset green.
  - leetcode: `contracts/` to `domain/`, `hooks/` to `state/`, `plan/hooks/` to `plan/state/`.
  - coding-agent: `integration/` to `state/`, `contracts/` into `domain/`.
  - service-report: `contracts/`, `use-cases/` and `lib/` into `domain/`.
  - dashboards: `git mv components/dashboards features/dashboards`, with a `project.json`.
- 2.4 **Switch every package to its preset**, with a baseline written by `depcruise-baseline` and `--ignore-known`. This is the only unit that switches presets; later units only shrink baselines.
  - mla-tamagui's `lint` gains `depcruise app app-shell lib features/leetcode features/tamil-writing features/dashboards --config .dependency-cruiser.cjs --ignore-known` (about 2.2 s in the app assessment, unreproduced).
  - Delete the `arch` script (`frontend/mla-tamagui/package.json:13`) and its step (`architecture.yml:108-109`), and drop the `arch` command from root.md.
- 2.5 **Rust service rules.**
  - Rewrite the four `backend/arch-rules` rules to match the layer identifier anywhere in a `use` or path.
  - Replace the glob ignore (`application-does-not-reach-outward.yml:9`) with the five named files: attachments, edit_stats, one_shot, runs, voice_planner.
  - Add `inner-rings-do-no-io` and `api-does-no-io`, with only the rca files the services assessment found as ignores:
    - inner rings: `core/src/domain/ids.rs`, the five application files, and `rs/src/domain/{clis,models,util}.rs`;
    - api: `app.rs`, `ws/methods.rs` and `routes/{voice,inventory,file_preview,code_assist,tamil,diff,worktree,plan}.rs`.
  - Add `testConfigs` and `arch-tests/` to `backend/sgconfig.yml`, and append `ast-grep test … --skip-snapshot-tests` to `arch:services`.
  - The `service-architecture` glob (`lefthook.yml:43`) gains `backend/arch-tests/**`.
- 2.6 **CLI rules.**
  - `clis/sgconfig.yml`, `clis/arch-rules/` (`core-does-no-io` for rca-core now; the wf rules land with wf-core) and `clis/rule-tests/`.
  - `arch:services` gains the scan and the test.
  - The `service-architecture` glob gains `clis/**/*.rs`, `clis/sgconfig.yml`, `clis/arch-rules/**` and `clis/rule-tests/**`.
- 2.7 **ls-lint** for `libraries/hook-runtime/src`, `tooling/browser-harness/src`, `scripts/` and `clis/wf-core/src`, with their paths added to the same glob.
- 2.8 **Rule-set guards**, a `wf check --arch` step that `wf check --staged` also runs, with a flow test in `clis/wf/tests/flow/check.rs`:
  - The ratchet only shrinks. For each `.dependency-cruiser-known-violations.json` and each `arch-rules/*.yml` `ignores` list, the entry count is at most the count at `git merge-base HEAD origin/dev`, and no ratchet entry contains `*`. Given a planted baseline with one more entry, when the check runs, then it exits 1.
  - Every rule is tested. Every `arch-rules/*.yml` id has an `arch-tests/<id>-test.yml` (or a `clis/rule-tests/` file) with at least one `valid` and one `invalid` case, and `ast-grep test` reports `N passed` with N equal to the rule count. `ast-grep test` exits 0 with "0 passed" when no test exists (ast-grep 0.45.3, 21 Oct), so the exit code alone proves nothing. Given a planted rule file with no test, when the check runs, then it exits 1.

Done when:
```sh
pnpm ls -r --json | jq -e 'map(.name) | index("@lib/feature-kit")' >/dev/null \
  && pnpm --fail-if-no-match --filter @lib/feature-kit run test:unit && pnpm run arch:services \
  && wf check --arch \
  && ast-grep test --config backend/sgconfig.yml --skip-snapshot-tests \
  && ast-grep test --config clis/sgconfig.yml --skip-snapshot-tests \
  && pnpm --fail-if-no-match --filter mla-tamagui run lint \
  && ! jq -e '.scripts.arch' frontend/mla-tamagui/package.json \
  && ! git grep -nF 'application/*.rs' -- backend/arch-rules
```

Risks:
- **dependency-cruiser depends on the TypeScript 6 API.** It refuses TypeScript 7 (`depcruise.feature.cjs:11-14`), and `pnpm-workspace.yaml:29-31` says that API package goes once 7.1 ships one. When it goes, every dependency-cruiser rule stops. The refusal is loud and the planted test fails, so the break cannot pass silently.
- **A casually regenerated baseline hides new debt.** 2.8 rejects a baseline that grew; a regeneration that keeps the count still needs review.
- **The rewritten Rust rules could flag more than today's debt.** The services assessment reported that on a scratch copy they flag exactly the current rca files (unreproduced). If the real run finds more, the unit stops and the finding is reported.

### Phase 3. The app features and the shell

Packages, in order:
- leetcode first: it is the reference.
- tamil-writing next: `feature-di-migration.md` names it first.
- then dashboards, coding-agent, service-report, settings, dev-tools, leetcode-contracts and the shell.

Units:
- **First, for every feature**
  - A0. `@lib/app-di`: `overrideKey<T>(name)` and a typed `useDependencyOverrides(key)`. It lands before any container, so each Phase 3 container is written once with its typed key; the string overload goes once no string key is left.
  - F0. The flow run collects in-app feature tests. Today `FLOW_TEST_GLOB` is `{src,tests}/**/*.flow.test.{ts,tsx}` relative to the package (`frontend/libraries/flow-test-harness/src/vitest/flowTestConfig.ts:24,270`), so a flow test moved into `features/leetcode/` leaves the flow run, and the unit config picks it up through `'features/**/*.test.{ts,tsx}'` (`frontend/mla-tamagui/vitest.config.ts:118`). Widen mla-tamagui's flow include to `features/*/**/*.flow.test.{ts,tsx}` minus the package features, add `**/*.flow.test.*` to the unit config's exclude, and drop `--passWithNoTests` from `test:flow` (`frontend/mla-tamagui/package.json:36`). Record the collected flow-test count (vitest's JSON reporter, `numTotalTests`); every later Phase 3 unit gates on it not falling.
- **leetcode** (its directory renames landed in 2.3)
  - L1. Move `data/problemCatalog.ts` (with `blind75.json`) and `data/voiceEditIdentity.ts` into `domain/`, and `VoiceEditTransportError` into `domain/ports.ts`. Delete their ratchet entries (at HEAD the `pathNot` exemptions at `.dependency-cruiser.cjs:54-57`). This removes all six ui and state → data edges.
  - L2. Add `state/ports.tsx`. `di/LeetCodeContainer.tsx` mounts `ExpoAudioRecorderProvider`, which leaves `routes/_layout.tsx:7`.
  - L3. After F0, move the 3 hook flow tests (`tests/flow/useLiveTranscript`, `usePlanRealize`, `useVoiceEditPseudo`) and the 3 tests in `tests/features/leetcode/ui/__tests__/` into the feature.
  - L4. Fix the `index.ts:22` comment.
  - `leetcode-ui-direct-io-debt.md` closes after L1. `MermaidDiagram.tsx` keeps its WebView in `ui/` by rule.
- **tamil-writing**
  - T1. `data/tamilCourse.ts` moves to `domain/course.ts`.
  - T2. Add `domain/ports.ts` (`TamilWritingPorts { writingCheck }`), `createHttpWritingCheck(baseUrl, fetchImpl)`, `state/ports.tsx`, and `di/TamilWritingContainer.tsx` mounted in a new `routes/_layout.tsx`.
  - T3. `testing/` with a fake check. Replace the `vi.spyOn` at `tests/flow/tamilWriting.flow.test.tsx:247-248` with an override, and, after F0, move all three tamil flow tests (`tamil-strokes`, `tamilWriteInsets`, `tamilWriting`) into the feature.
  - T4. routes import ui relatively, not through `'@/features/tamil-writing'`.
  - T5. Move `lib/skiaWeb` into a library.
- **dashboards** (moved to `features/dashboards` in 2.3)
  - D1. Split it into rings: screens in `ui/`, the schema in `domain/`, the port in `domain/ports.ts` and the adapter in `data/`.
  - D2. Add `di/DashboardsContainer.tsx`; all three screens use the hook, the index screen included.
  - D3. Move `backTo` out of settings into the design system, with its 3 dashboard imports.
- **coding-agent** (its directory renames landed in 2.3)
  - C1. Add `domain/ports.ts` (`CodingAgentPorts { orchestration, dispatchRuns, tokenStore }`).
  - C2. Add `state/ports.tsx` and `di/CodingAgentContainer.tsx`. Move `app/coding-agent/[threadId]/settings.tsx` under a tree whose layout mounts the container, keeping its `transparentModal` presentation (`app/_layout.tsx:124-126`). Delete the transport inference in `ThreadListScreen.tsx:103-104` and `DispatchRunScreen.tsx:56-58`, and the fixture branch in `useDispatchRuns.ts:54-56`.
  - C3. `data/fixtureGateway.ts`, the fixture dispatch-runs gateway and `src/fixtures/` move to `src/testing/`; coding-agent ships no fixture mode, and its capture scripts use `testing/node`.
  - C4. `ThreadSettingsSheet` uses the token-store port.
  - C5. Empty coding-agent's baseline and trim the barrel.
- **service-report** (its directory renames landed in 2.3)
  - S1. Add `ServiceReportPorts { healthReports, goldenComments }`, adapter factories, and the ORDS env read moved into `@lib/app-config`.
  - S2. A container; the flow test overrides the port; a golden-comments flow test.
  - S3. Delete the duplicate `tests/flow/serviceReport.flow.test.tsx`.
- **settings**: empty its baseline, and replace the `vi.spyOn` on `usePresetThemeStore` (`tests/flow/settings.flow.test.tsx:54`) with an assertion on the store state.
- **dev-tools**: move `AgentationOverlayHost` into `app-shell/` and delete the package, or keep it as type 9 (open question).
- **leetcode-contracts**: `src/contracts/*` moves to `features/leetcode/domain/wire/`; rewrite the 13 imports; remove the package.
- **shell**: the `showcase.tsx` and `charts-spike.tsx` bodies move to `app-shell/`. A shell flow test renders every feature route through `createFlowApp` and fails on a missing provider. `feature-di-migration.md` closes when the hexagonal baselines are empty.

Done when:
```sh
jq -e 'length == 0' frontend/mla-tamagui/.dependency-cruiser-known-violations.json \
  && for p in coding-agent service-report settings; do \
       jq -e 'length == 0' "frontend/mla-tamagui/features/$p/.dependency-cruiser-known-violations.json" || exit 1; done \
  && pnpm --fail-if-no-match --filter mla-tamagui run test:flow \
  && ! test -d frontend/mla-tamagui/components \
  && ! git grep -nE "['\"]((\.\./)+|@/)features/[a-z-]+/(src/)?(data|state|di|hooks|plan|ui)/" -- frontend/mla-tamagui/tests
```
The last grep matches module specifiers only. At HEAD it finds the six test files L3 and T3 move, and not the logpoint path strings in `tests/features/leetcode/ui/__tests__/ProblemEditor.flow.test.tsx:240,252`. The flow-test count gate from F0 runs with `test:flow`.
Then publish an EAS Update with `wf ship eas`, and run the browser e2e tests against a local `expo export --platform web`.

Risks:
- **Moves touch many import lines.** Typecheck catches every miss.
- **Behaviour.** The feature flow tests and the editor-typing, voice-edit and tamil-writing e2e tests cover it.
- **coding-agent's WebSocket lifecycle moves into `di/`.** Run its flow tests and `frontend/mla-tamagui/e2e/verify-coding-agent-*.mjs` after each unit.

### Phase 4. The services

Packages: remote-chat-agent, vm-ops-api, leetcode-backend, leetcode-auth-cookie-capture, reporting-service, watchdog.

Units:
- **remote-chat-agent.** VERIFY for each unit: `pnpm run arch:services && pnpm --fail-if-no-match --filter @app/remote-chat-agent run test:flow`.
  - R1. Replace the `infrastructure::log` wrapper with `tracing` macros, keeping the output shape: message is the event, `meta` is the object. The api ignores go from 12 to 7.
  - R2. `highlight.rs` and `tamil.rs` move to `rs/src/domain/`, and `docx_text` to `domain/docx.rs`. The ignores go to 5.
  - R3. The opencode `Client` moves to `application/opencode.rs`; `Http` stays in infrastructure. The ignores go to 3.
  - R4. A `DocumentText` port; the application ignores are emptied.
  - R5. A `SpeechToText` port that owns the temp files. The ignores go to 1.
  - R6. `Config` moves to `infrastructure/config.rs`, and adapter construction moves from `api/app.rs` to `main.rs`. The api ignores are emptied.
  - R7. Env becomes a typed `Settings`, one module per commit. The flow harness's `SERVICE_ENV` shrinks each time and its mutex goes.
  - R8. fs goes behind ports: `DiffSnapshots`, `WorkspaceFiles`, worktree removal, and the `RunHost` implementation in `infrastructure/run_host.rs`.
  - R9. `Clock` and `Ids` ports in the core.
  - R10. The `code_assist` mock becomes a fake adapter, or the route goes.
  - `docs/plans/rca-hexagonal/plan.md` closes after R6; R7 to R9 carry the rest of the type-1 debt (env reads, fs, the core clock and ids).
  - Status 3 Nov: R7 done (`application/settings.rs` is the only env reader; the domain reads none; 2874f53dc). R8 done: `DiffSnapshots` and `WorkspaceFiles` ports in core, adapters `infrastructure/diff_snapshots.rs` and `workspace_files.rs`; the api and application layers no longer touch `std::fs` (c743a842c, ee5ec79e2, 3c67c2f61, 20fa17d10). R9 holds: `Clock` and `IdSource` are core ports; the remaining `Instant::now` calls are tokio deadlines. R10 done: the code-assist mock route went (96e2e2f78).
- **vm-ops-api.** Ported to Rust in `backend/vm-ops-api/{core,rs}` (`docs/plans/vm-ops-api-rs/plan.md`), which takes the type-1 shape; its integration suite, `backend/vm-ops-api/integration/`, starts the real service and talks to it over HTTP, and the done-when below runs it with `test:integration`.
- **Small fixes.**
  - capture: `WF_BIN` moves into `Config`.
  - reporting-service: `Ports` moves to core `application/ports.rs`; `commit` becomes a setting.
    Done 3 Nov (f5d4cd9ce); capture's `WF_BIN` is a `Config` setting (10fc68cd0).
  - watchdog: document the no-api shape and the seam rule; the packaging decision is an open question.

Done when:
```sh
pnpm run arch:services && ! git grep -n 'remote-chat-agent/' -- backend/arch-rules \
  && pnpm --fail-if-no-match --filter @app/remote-chat-agent run test:flow \
  && pnpm --fail-if-no-match --filter @backend/vm-ops-api run test:integration
```
Then each service's `test:verify:deployed` passes in CI, after confirming that the remote head equals the pushed commit.

Risks:
- **rca regressions surface late.** The 314-test TypeScript black-box suite runs only for benches, so the net is rca's flow suite plus `test:e2e` and `test:verify:deployed`.
- **R1 can break BetterStack queries.** They read the log shape, so R1 keeps it byte-equal.
- **R6 and R7 can change config defaults.** Keep env names and defaults, and add one round-trip test.
- **R9 may disturb goldens that pin ids.**
- **vm-ops-api ships first and redeploys the others.** A broken primary blocks every deploy leg. The Rust port (`docs/plans/vm-ops-api-rs/plan.md`) carries this risk with a watchdog rule plus stop and freeze drills as the second door and, when the service stays down, a fix or a Terraform recreate of the VM (no standby and no fallback: owner, 23 Oct).

### Phase 5. Libraries, contracts and tooling

Packages: logging, app-di, flow-test-harness, app-config, rn-agentation, design system, gesture-geometry, voice-edit-dsl, eslint-plugin-mobile-learn, remote-chat-agent-contracts, hook-runtime, browser-harness, provision-apps, rca-core, rca-file-store, agent-config-sync, tooling/nx.

Before any unit, the owner answers open questions 7 and 8, and a deletion they choose lands first, so no port is built for code that goes. cli-http gets no `Transport` trait in this phase: no named flow test needs one (type 6).

Units:
- **logging**
  - Move the `Stage` type into `primitives`.
  - `ScrubberOptions.env` replaces the global `process.env` mutation in its test.
  - Add `src/testing/captureLogs.ts` and the `./testing` export.
  - Drop the unused `./logger` and `./plugins` subpaths.
- **app-di**: its typed keys landed in Phase 3 (A0); here only its `dist/` goes (below).
- **flow-test-harness**
  - `src/doubles/` moves into leetcode's `testing/`, typed against `LeetCodePorts`. `testing/` re-exports `data/fixturePorts.ts`, which stays in `data/` because the shipped fixture mode (`di/fixtureOverrides.tsx:4-8`) imports it.
  - The harness README's "A double" section (`frontend/libraries/flow-test-harness/README.md:190-210`) loses its move note; `src/index.ts` exports no double.
  - `setup.ts` mounts `captureLogs`. This closes `docs/gaps/flow-tests-cannot-assert-logger.md`.
- **app-config** (type 11 afterwards): what open question 7 keeps becomes a pure `resolveAppConfig({ env, launchArgs, devFile, allowedRoutes })`. The literal `process.env.EXPO_PUBLIC_X` object moves to the app's composition root and is passed in; the 9 self-mocks go with their disable comments.
- **rn-agentation**
  - Fix the 8 oxlint errors.
  - Add the KeyValueStorage port, and AnnotationSink only if open question 8 keeps the HTTP sink, with adapters in `adapters/`. The clipboard stays a direct `expo-clipboard` call (choice 5).
  - Choose the driver by file fork.
  - Add `src/testing/`.
- **design system**: add `lib/keyValueStore.native.ts` over `@react-native-async-storage/async-storage`, declared as a peer dependency as rn-agentation does. No setter port: the persisted stores hydrate at import (type 13), and a remount does not reset a module-level zustand store, so a remount test passes with or without a fix. The proof: write the theme key to the harness AsyncStorage stub, point the store at the native storage, call `usePresetThemeStore.persist.rehydrate()` and assert the theme; it fails without the fork.
- **gesture-geometry, voice-edit-dsl, app-di**: exports point at `src`; untrack `dist/`; add `frontend/libraries/*/dist/` to `.gitignore`.
- **eslint-plugin-mobile-learn**: move `Rule` into `rules/types.ts`. `no-unhandled-catch` accepts a catch whose returned value is derived from the error (`voice-edit-dsl/src/use-cases/pseudoCode.ts:307`).
- **remote-chat-agent-contracts**
  - `.` points at `src` once Metro resolves it in `export:web`.
  - Add golden fixtures parsed by zod and by a cargo test in rca.
- **hook-runtime**
  - Split `guard.mjs` into `src/core/guard.mjs` and `src/adapters/node.mjs`, and `opencode-adapter.mjs` into core plus adapter.
  - Rename the spawning tests to `*.integration.test.ts`.
  - Move `.agents/hooks/rtk-rewrite.sh` in.
  - Delete `mock-opencode-bin`, or move it to rca-core.
- **browser-harness**
  - Add unit tests of the reducers and geometry first.
  - Make `tooling/browser-harness/**` an input of `HARNESS.browser` (`tooling/nx/e2e-tests.js:23-28`) and of crate-wf's `cargo-flow`, and build `dist` in CI's flow-tests job.
  - Collapse the six packages into one, with `src/core`, `src/adapters`, `src/ui` and `src/main.tsx`.
  - Use one device registry: import `clis/wf/src/browser_device_specs.json`.
  - Update `nx_cache.rs:15,82,195,336,344-346,362` with each unit.
- **provision-apps**
  - Extract it into `infra/oracle-cloud/provision/provision-apps.mjs`, inlined by `file()`. Prove the rendered `user_data` is byte-identical with `terraform console`.
  - Then split it into core and adapters, with unit and flow tests.
- **rca-core**: `file_store.rs` (56 lines) moves to a new adapter crate `clis/rca-file-store` (type 6), which wf's adapters and rca-service's infrastructure depend on; rca-core keeps no I/O, checked by `core-does-no-io`; a reusable fake `RunHost` in `testing`.
- **agent-config-sync**
  - `contract.rs` splits into `parse(text)` and `load(root)`.
  - Add the `acs-pure-modules` rule.
  - Rename `src-rs` to `src` (`Cargo.toml:9,13`; `.agents/instructions/.agents.md:23-24`).
- **tooling/nx**: a test that every `HARNESS` glob matches a tracked file.

Done when:
```sh
pnpm -r --if-present run lint && pnpm -r --if-present run test:unit \
  && pnpm --fail-if-no-match --filter @lib/flow-test-harness run test:unit \
  && test -z "$(git ls-files 'frontend/libraries/*/dist/*' | grep -v '^frontend/libraries/logging/')" \
  && test "$(git ls-files 'tooling/browser-harness/*/package.json' | wc -l)" -eq 0 \
  && cargo test --manifest-path clis/Cargo.toml -p rca-file-store \
  && ! git grep -nE 'std::fs|use std::\{[^}]*\bfs\b' -- clis/rca-core/src
```
Then the browser e2e job passes with the collapsed device frame. A VM recreate is the runbook for provision-apps: it is pre-authorized, and it is the only real proof, because `main.tf:264` ignores metadata changes.

Risks:
- **hook-runtime guards every Bash, Edit and Write call.** A broken split blocks the session or opens the guard. Keep the committed deny cases (`guard.test.ts:50-99`) green at each step.
- **The device frame is a contract with wf.** wf depends on its dist path, base URL and `__shell` method names.
- **cloud-init is the only way the VM comes back by itself.**
- **The design system rehydrates late on native.** zustand persist rehydrates asynchronously, so the first frame can show the default theme.

### Phase 6. wf

Packages: wf, wf-core, and the login gate's `clis/vnc-auth-capture/{core,listener}` crates.

Units:
- W1. **A lib target.** The module tree moves to `src/lib.rs`, and `main.rs` calls `wf::run`.
- W2. **The run seam.** Add `Io`, `World` and `wf::run(argv, &World, &mut Io) -> i32`. `routes.rs`'s handler becomes `Legacy(fn) | Ported(fn(&[String], &World, &mut Io))`. Write the first in-process flow test, for `--version`. Add the `wf-io-outside-adapters` budget at that day's count.
- W3. **The `clis/wf-core` skeleton**, before any port exists: workspace member, dependency allow-list, `compile_fail` doctest, `testing` feature, `src/ports.rs`, ls-lint entry, and the wf ast-grep rules (`wf-cli-does-no-io`, `wf-cli-uses-no-adapters`, `wf-adapters-use-no-cli`) with their rule tests. The doctest needs the lib target, so it runs from here on.
- W4. **One process runner.** `adapters/proc.rs` absorbs the four runners (`verify_tier3.rs:75`, `worker_run.rs:235`, `agent_proof.rs:100`, `secrets/backend.rs:16`), one caller file per unit.
- W5. **The first adapters:** git, adb (five spawn sites), oci and vmops under `src/adapters/`. Only `Repo` and `VmOps` become ports, in `wf-core/src/ports.rs`; adb and oci serve thin commands and stay port-less (the CLI variant rule).
- W6. **The already-pure modules** move into wf-core, one unit each: `build_image.rs`, `trace.rs`, `pixelmatch.rs`, `app_bench/domain.rs`, `device/sel.rs`, `device/api.rs`.
- W7. **The families, in order:** ship, test, check, device, ask/plan, secret, login gate, browser, android, vm, repo, log and shell. Each family PR:
  - moves its rules into wf-core and its handlers into `cli/`, and its I/O into `src/adapters/`;
  - adds in-process flow tests, and contract tests for each new port in `tests/flow/contract.rs`;
  - prunes only the black-box cases those tests now cover;
  - lowers the budget.
  - The test family's Metro port removes the six curl spawns at `e2e_tests.rs:417-470` and the `WF_TEST_*` switches.
  - The login gate moves `vnc/` into the crate pair `clis/vnc-auth-capture/core` and `clis/vnc-auth-capture/listener` (type 1); wf depends on the core crate. This deletes the `#[path]` includes and the wf-source COPY lines (`Dockerfile:23-26`, `deploy.rs:68-77`, `tests/flow/ship.rs:95-173`).
- W8. **Config is read once in `world.rs`.** The `set_var` sites go as their families port (`main.rs:117`, `env_file.rs:115-138`, `run.rs:86-131`, `ship_eas.rs:199`, `secrets/mod.rs:485`, `usage.rs:88`).
- W9. **Finish.**
  - Delete `Legacy`.
  - Turn on the ls-lint end state for `clis/wf/src` (`src`, `cli`, `adapters`, `help` dirs; `lib`, `main`, `world` files).
  - Rename and prune the black-box suite (open question).
  - Correct `requirements.md:269` and `standard-crates.md:101`.

Done when:
```sh
wf check --budget \
  && test "$(git ls-files 'clis/wf/src/*.rs' | grep -E '^clis/wf/src/[^/]+\.rs$' | grep -cvE '/(lib|main|world)\.rs$')" -eq 0 \
  && cargo test --manifest-path clis/Cargo.toml -p wf-core --lib \
  && cargo test --manifest-path clis/Cargo.toml -p wf-core --doc \
  && cargo test --manifest-path clis/Cargo.toml -p wf --test flow \
  && ast-grep test --config clis/sgconfig.yml --skip-snapshot-tests && wf check --arch && pnpm run arch:services
```
`wf check --budget` includes `wf-io-outside-adapters`, which reaches 0 only when the I/O has moved into `src/adapters/`; the file-count line alone would pass after a plain `git mv` into `cli/`. The contract tests run inside `--test flow`.

Risks:
- **A wf regression breaks CI one push later.** The pipeline's test jobs run the wf the previous push published. Mitigations:
  - one family per PR, with no behaviour change mixed in;
  - the black-box suite stays the oracle until flow tests cover the family;
  - compare cold start and binary size per family.
- **The login gate is exposed on the tailnet.** Re-probe 401, 403 and 202 through the tailnet before merging (`standard-crates.md:95`, at HEAD).

## Verification

Every rule set is proven by a planted violation that must fail, and by a clean twin that must pass.

| Rule set | Planted proof | Command | Pre-commit | CI |
|---|---|---|---|---|
| dependency-cruiser presets | `frontend/feature-kit/fixtures/planted/<preset>/dirty` holds exactly one offending import per rule: domain→react, data→state, state→data, ui→data, ui→`@lib/app-di`, routes→data, feature→`@/lib/x`, feature→other feature, a module in `helpers/`, a cycle, core→expo-clipboard, core→`node:fs`, a library→`frontend/mla-tamagui`. The test runs `cruise()` with the real configs; the dirty tree reports exactly those rule names and the clean twin reports none. | `pnpm --fail-if-no-match --filter @lib/feature-kit run test:unit` | `wf check --staged`, when a feature-kit file is staged (2.1 makes it a workspace package) | typecheck-lint-unit-tests; `frontend/feature-kit/**` is in the `archRules` input of every `lint` (2.1), so a change re-runs every `lint` (today only `depcruise.feature.cjs` is, `nx.json:12`) |
| oxlint arch overrides | in the same suite: `fetch()` in `ui/`, AsyncStorage in `ui/`, `process`, `globalThis.process`, `node:fs` and a bare `fs` import in `src/core/`, an adapter import from `src/ui/`, `vi.spyOn` in a flow test, `vi.mock` in a library test; `node:path` and `node:crypto` in core pass | same | same | same |
| ls-lint layouts | the same suite runs ls-lint with the repo's `.ls-lint.yml` on a temp tree holding `scripts/foo.sh` and `clis/wf-core/src/Bad.rs` | same | `service-architecture` job | via `lint` |
| ast-grep, Rust services | `backend/arch-tests/<id>-test.yml`, `invalid` snippets: `use crate::infrastructure::log;`, `use crate::{api, infrastructure::log};`, `use super::super::infrastructure::stt;`, `std::fs::read`, `std::env::var`, `Uuid::new_v4`; `valid` snippets: `tokio::time::sleep`, a core path | `ast-grep test --config backend/sgconfig.yml --skip-snapshot-tests`, and `wf check --arch` for the test count | `service-architecture` job (glob includes `backend/arch-tests/**` after 2.5) | each service package's `lint`, re-run by the `archRules` input |
| ast-grep, CLIs | `clis/rule-tests/`: `std::process::Command::new("git")`, `use std::{fs, io};`, `std::env::var("X")`, `std::time::Instant::now()`, `cli_http::Request::get(u)` are invalid, in wf-core and (for `wf-cli-does-no-io`) in `cli/`; `fn f(out: &mut dyn std::io::Write)` and `use std::collections::HashMap;` are valid | `ast-grep test --config clis/sgconfig.yml --skip-snapshot-tests` | `service-architecture` job (glob widened) | crate `lint` kind |
| Cargo walls | a `compile_fail` doctest in `wf-core/src/lib.rs` that imports `cli_http` and `cdp`, which wf-core's allow-list excludes (rca-core gets none: it never depended on them) | `cargo test --manifest-path clis/Cargo.toml -p wf-core --doc` | post-land `rust-clis` (glob widened in 1.4) | crate `unit` kind, second command |
| `wf-io-outside-adapters` budget | a flow test plants a `Command::new` in `clis/wf/src/cli/` of a temp tree and expects exit 1 | `cargo test --manifest-path clis/Cargo.toml -p wf --test flow check::` | `wf check --budget` | crate `flow` kind |
| rule-set guards (2.8) | a flow test plants a known-violations baseline with one more entry, a `*` ignore, and an ast-grep rule with no test; each must exit 1 | `cargo test --manifest-path clis/Cargo.toml -p wf --test flow check::` | `wf check --staged` | crate `flow` kind |
| lint reaches every package | flow test 1.1 | same | — | same |
| config contracts | today's red healthcheck test is the live proof until 1.5 fixes it | `pnpm --fail-if-no-match --filter @infra/oracle-cloud run test:unit` | via `wf check` | typecheck-lint-unit-tests |

What CI runs after Phase 2:
- **flow-tests:** every package's `test:flow` and every crate's `--test flow`, wf's black-box suite included.
- **typecheck-lint-unit-tests:**
  - every package's `typecheck`, `lint` and `test:unit` (oxlint plus the package's dependency-cruiser preset, or `arch:services` for the services, plus the feature-kit planted suite);
  - every crate's `unit` and `lint` (clippy plus the clis ast-grep scan).
- **deployment-verification-tests, browser-e2e-tests, android-e2e-tests:** unchanged.
- **Pre-commit:** `wf check --staged` reaches every package's `lint` when a JavaScript or TypeScript file of it is staged, and runs `wf check --arch`; the `service-architecture` job covers the Rust sources, rule files and rule tests of `backend/` and `clis/`.
- **`architecture.yml`:** stays a manual full run, without the `arch` step.

Each unit reports the tests that ran in the repo's order: runbook, browser and Android e2e, deployment verification, integration, flow, unit. Rule-only units (Phases 1 and 2) report flow and unit, and say so. A UI unit shows its screenshots. Each recordable run publishes its recording and its bundle per CLAUDE.md.

## Open questions

Each question carries this spec's recommendation.

1. **The archived pair.** Keep `docs/archive/solutions/architecture/clean-architecture.md` and its twin as history (recommended: the new doc names them as replaced), or delete them?
2. **Ring names.** Rename feature directories to the ring names (`hooks/` and `integration/` to `state/`; `contracts/`, `use-cases/` and `lib/` to `domain/`; about 30 file moves, done in 2.3 before any baseline, recommended), or let the presets accept per-feature aliases?
3. **Device services and persistence.** No port for a device service until a test must drive it (in flow tests they run through react-native-web or their web build), and zustand persist with AsyncStorage in `state/` (recommended)? Or a port for every one?
4. **@feature/dev-tools.** Dissolve it into `app-shell/` (recommended: its DI is dead and its overlay renders null), or keep it as a presentation feature?
5. **@lib/leetcode-contracts.** Fold it into `features/leetcode/domain/` (recommended: one consumer), or keep it for an expected second consumer?
6. **Dead packages.** Delete `@lib/registry-service` (no importer; wf owns the slot registry) and `@tooling/wf-blackbox` (its one test reads a file deleted in 7dbaa9b63)? Unit 1.2 assumes yes.
7. **The initial route.** Nothing in the app reads `appConfig.initialRoute`, so `wf ship eas --initial-route` bakes a value no router uses. Wire `app/+native-intent.ts`, or delete the machinery in `@lib/app-config` (about 190 source and 540 test lines)?
8. **rn-agentation's HTTP sink.** Wire `appConfig.agentationEndpoint`, or delete the sink and the runbook claim at `docs/runbooks/rn-agentation.md:126`?
9. **Tracked `dist/` in frontend libraries.** Switch to source exports and untrack it (recommended)? The logging babel build stays, because Node needs it.
10. **Arch rules in CI.** Run them through each package's `lint` (recommended: it rides existing gates), or as one root step in `test_plan.rs`?
11. **clippy.** The 21 Oct count is 9 findings, all in wf (see Phase 1 risks). Fix them in 1.4 and run `-D warnings` on every changed CLI crate in CI (recommended), or keep a fixed allow-list?
12. **wf-core.** Its own crate (recommended: a Cargo wall, and fast unit builds without mlua's vendored C), or a module guarded only by ast-grep?
13. **wf's black-box suite.** Rename `tests/flow.rs` to `tests/blackbox.rs`, so reports name the integration tier?
14. **vm-ops-api's composition root.** Closed 23 Oct: the service is Rust, so the composition root is `backend/vm-ops-api/rs/src/main.rs`; the Node `main()` went with the Node source (S4).
15. **watchdog.** Move `package.json` and the Dockerfile into `backend/watchdog` so its crates have a `lint` gate, or keep the split with pre-commit-only rules?
16. **rca's core clock and ids.** Add `Clock` and `Ids` ports (recommended, R9), or record them as an accepted exception?
17. **The login gate's vault fallback.** If env is enough, the listener drops all secrets code. Otherwise `clis/secrets` becomes a crate shared by wf and the listener.
18. **Agent hooks.** Keep them in `libraries/hook-runtime` (recommended: they run before wf is built), or port them into wf?
19. **The check-eas-published Stop hook.** Closed 24 Oct: deleted with its script, the `SKIP_PUBLISH_CHECKS` default and the Stop and `session.deleted` hook entries.
20. **provision-apps.** A tested Node module inlined by `file()` (recommended), or a `wf vm provision` verb? And should `retry-apply.sh` move into `wf vm terraform`, so a retry passes the free-tier guard?
21. **A CI integration stage.** Add `test:integration` to the plan, or keep process-spawning smokes in `test:flow` with an `*.integration.test.ts` suffix?
22. **Outside this spec.** `EXPO_PUBLIC_HEALTH_REPORTS_ORDS_PASSWORD` is inlined into the client bundle (`service-report/src/data/ordsClient.ts:55`). This needs a security decision; the migration only moves the read into `@lib/app-config`.
