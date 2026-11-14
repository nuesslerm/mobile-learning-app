---
title: Hexagonal architecture by package type — one dependency rule, a shape per package type, and the checks that hold it
date: 2026-10-21
updated: 2026-10-24
category: architecture
module: repo-root
problem_type: architecture_pattern
component: development_workflow
severity: medium
tags: [architecture, hexagonal, ports-and-adapters, composition-root, dependency-overrides, dependency-cruiser, ast-grep, ls-lint, package-types]
applies_when:
  - "adding a module, port, adapter or test double to any package"
  - "adding a package or a feature, or choosing its shape"
  - "adding or changing an architecture check"
related_components: [frontend, service_layer, tooling, testing_framework]
---

# Hexagonal architecture by package type

This is the repo's one architecture doc: the dependency rule, the shape of each package type, what holds today and how it is checked (with the command), the target, and which test covers which ring. The plan that moves every package from today to the target, unit by unit, with its open questions, is `docs/plans/hexagonal-architecture/spec.md`; its type numbers match the ones below. An owner decision recorded in that plan can change a target here. Where an older doc or plan disagrees with this one, this one wins.

## The dependency rule

Every dependency points inward.
- **The core does no I/O.** The core is the domain plus the application. It touches no file, network, process or environment, and (target) no wall clock or randomness: those come in as arguments or ports. Logging through `Logger.*` (TypeScript) or `tracing` (Rust) is the one allowed ambient effect.
- **The core declares a port for each outside capability it needs**, named in its own words (`SessionStore`, `MetricsStore`, `LeetCodeGatewayPort`), never after the technology.
- **A driven adapter implements one port over one technology.** A driving adapter (router, CLI handler, screen, route module, Lua binder) turns input into a use-case call.
- **The composition root is the only module that names a concrete adapter.** It reads configuration once, builds the adapters and hands them in. It builds the graph; the entry point runs it (leetcode: `di/LeetCodeContainer.tsx` builds, `routes/_layout.tsx` mounts).
- **The package that owns a port ships its double**, in its own testing module. There are no `-mocks` or `-impl` sibling packages.
- **A package has only the rings it needs.** A package with no I/O has no ports; a package that is all I/O has no core.
- **No DI framework.** A TypeScript port is a type and its adapter a factory function (`createLiveLeetCodeGateway(baseUrl)`), not a class. A Rust port is a trait. Wiring is plain code.

## Vocabulary

- **Ring**: one layer, from the inside out: domain, application, adapters, composition root. **Port**, **adapter** and **composition root** are as in the rule above; a **double** is a fake implementation of a port.
- **Transport seam**: a narrower trait an adapter defines for its own tests (watchdog's `DockerCommand` under its Docker adapter). The application never names it.
- **Ratchet**: named files exempt from a rule while they migrate (ast-grep `ignores`, a dependency-cruiser known-violations baseline). It only shrinks; a glob is never a ratchet entry.

Every package type takes one of six shapes. The Rings column of the package-types table names it, and the plan uses the same names.
- **Full hexagon**: domain, application with its ports, driven and driving adapters, a composition root, doubles. Types 1, 3, 8, 12 (type 2 is empty).
- **Functional core, imperative shell**: one core (domain and application merged) whose ports are its parameters, adapters for those parameters, and entry files as composition roots. Type 4.
- **Core only**: pure rules, port traits where a consumer supplies the I/O, doubles; no adapters. Types 5, 10, 11.
- **Adapter only**: flat modules over one outside system; no rules. Type 6.
- **Delivery only**: driving adapters (screens, routes, components, a driver API) and, where the package wires others, its composition root and the adapters it hands to library ports (the shell's `app/_layout.tsx` and `lib/`, the harness's vitest config and stubs); no domain or application. Types 7, 9, 13, 14.
- **None**: declarative or sequencing files with no decision. Types 15, 16.

Where the rings live in the full-hexagon types and in type 4:

| Ring | 1 Rust service | 3 Hexagonal CLI | 8 Hexagonal feature (target) | 4 Small tool (target) |
| --- | --- | --- | --- | --- |
| domain | `core/src/domain/` | the pure modules of a `wf-*` crate (planning, parsing, rendering) | `domain/` | `src/core/` |
| application | `core/src/application/` | the crate's command handlers, `fn(&[String]) -> Option<i32>` | `state/` (hooks, stores) | `src/core/` |
| ports | `core/src/application/ports.rs` | in the crate that owns them (`Device` and `Platform` in `harness-core`, `Backend` in `secrets`) | `domain/ports.ts` | parameter types in `src/core/` |
| driven adapters | `rs/src/infrastructure/` | the I/O modules of each crate, over `cli-http`, `cdp`, `wf-vmops` | `data/` | `src/adapters/` |
| driving adapters | `rs/src/api/` | `wf-kit`'s one dispatcher over each crate's `COMMANDS` | `ui/`, `routes/` | entry files, `src/ui/` |
| composition root | `rs/src/main.rs` | `clis/wf` (`main.rs`, `routes.rs`: every crate's commands and help); a thin `src/bin/wf-<name>.rs` per crate for its tests | `di/` | entry files at `src/` |
| doubles | `core/src/testing.rs` (feature `testing`) | feature `testing` of the crate that owns the port (`wf-vmops`, `harness-browser`); `wf-testkit` for the shared ones | `testing/` | `src/testing/` |

Ports sit in `application/` where an adapter may import the application ring (Rust services). In a feature, `state/` holds React hooks and stores that an adapter must not import, so ports sit in `domain/ports.ts`. Same sentence either way: the core declares the port in the innermost ring the adapters may import.

## Package types

Ask in order; the first yes decides. A declarative file a tool reads → 16. Only sequences other programs → 15. Deployed and runs on its own → 1. A CLI with rules and several outside systems → 3. A tool whose rules meet one or two outside things → 4. A Rust library of rules shared by several consumers → 5. All I/O against one outside system → 6. `frontend/mla-tamagui` outside `features/` → 7. A feature a user reaches: calls a backend or persists data → 8, otherwise 9. Only schemas and types that two or more packages consume → 10. A frontend or JS library: no I/O → 11, rules that wrap I/O → 12, rendering components → 13. Drives other packages' code under test → 14.

| # | Type | Rings | Members | Today |
| --- | --- | --- | --- | --- |
| 1 | Rust service | full hexagon, `core`/`rs` crates | leetcode-backend, leetcode-auth-cookie-capture, reporting-service, remote-chat-agent, watchdog, vm-ops-api as `backend/vm-ops-api/{core,rs}` (`@backend/vm-ops-api`); the login gate as `clis/vnc-auth-capture/{core,listener}` (target) | holds; remote-chat-agent carries a ratchet |
| 2 | TypeScript service | full hexagon, folders | none: empty since vm-ops-api moved to Rust (`docs/plans/vm-ops-api-rs/plan.md`); the number stays so the others keep theirs | n/a |
| 3 | Hexagonal CLI | crates along seams, ports in the crate that owns them | wf and the `clis/wf-*` crates | split into crates (23 Oct 2026); no layer rule inside a crate yet |
| 4 | Small tool | functional core, imperative shell | `@lib/hook-runtime`, `tooling/browser-harness`, provision-apps (to extract from cloud-init), agent-config-sync | agent-config-sync close; the others not yet |
| 5 | Core library | core only | rca-core | holds except its file store |
| 6 | Adapter library | adapter only | cdp, cli-http, telemetry, `@lib/service-kit`, `@lib/browser-client`; rca's file store (target) | holds |
| 7 | App shell | delivery only | `frontend/mla-tamagui` outside `features/` | partly; nothing checks it |
| 8 | Hexagonal feature | full hexagon, folders | leetcode, tamil-writing, coding-agent, service-report, dashboards | not yet (see below) |
| 9 | Presentation feature | delivery only | `@feature/settings` | settings holds (the dev-tools feature dissolved into the app shell: its DI wiring was dead) |
| 10 | Contracts package | core only | `@app/remote-chat-agent-contracts`, `@cli/harness-contracts` | partly; unchecked. `@cli/harness-contracts` has no zod and holds a device registry and geometry functions (`tooling/browser-harness/contracts/src/index.ts:209-444`). `@app/remote-chat-agent-contracts` (coding-agent) has one consumer; the leetcode-contracts library folded into `features/leetcode/domain` |
| 11 | Pure library | core only | `@lib/gesture-geometry`, `@lib/voice-edit-dsl`, `@lib/eslint-plugin-mobile-learn`, `@lib/app-di`; `@lib/app-config` (target) | holds, except app-config, which reads env at import; unchecked |
| 12 | Hexagonal library | full hexagon, folders | `@lib/logging`, `@lib/rn-agentation` | partly |
| 13 | Component library | delivery only | `@lib/tamagui-design-system` | holds; unchecked |
| 14 | Test harness | delivery only | `@lib/flow-test-harness` | ships leetcode's doubles |
| 15 | Glue | none | `scripts/postinstall.mjs`, `scripts/prepare.mjs`, `tooling/nx`, container scripts, `.agents/setup.sh`, the agent hook shims (`.agents/hooks`, `.opencode/plugin`), `.conductor/archive-workspace.sh`, the budget-guard function, the rca stt sidecar | `scripts/` still holds hook steps |
| 16 | Config and data | none | Terraform and the new `@infra/oracle-cloud` package, compose files, Dockerfiles and the vnc-auth-capture image files, `vm-apps.json`, `infra/tailscale/policy.hujson`, lint configs, `clis/secrets`, `clis/app-bench`, `frontend/feature-kit` | unchecked |

The `e2e/*.lua` files are test cases, not packages; wf's device harness runs them. The per-package assignment, with deletions and folds (the registry-service library, the wf-blackbox tooling package, the dev-tools feature and the leetcode-contracts library), is the plan's inventory.

### 1. Rust service

- `core` crate: `domain/` (pure), `application/` (use cases; `ports.rs` holds one trait per driven capability, 22 across the five older cores today, and vm-ops-api's are the table in `docs/plans/vm-ops-api-rs/plan.md`, section 4, "Ports"), `testing.rs` (fakes, feature `testing`). Pure dependencies only. `rs` crate: `infrastructure/` (one module per adapter, config via clap `env`), `api/` (thin axum handlers; `pub fn app(Ports, Settings) -> Router` shared by `main` and the flow tests, `backend/leetcode-backend/rs/src/api/mod.rs:60`), `main.rs` (composition root).
- Rule: `api → application → domain`; `infrastructure → application`; `api` names no infrastructure.
- No layer without a need: leetcode-backend has no `domain/`; watchdog has no `api/`. `backend/service-kit` is an adapter library used by `main`, `api` and `infrastructure`, never by a core.
- vm-ops-api (`backend/vm-ops-api/{core,rs}`) has a `domain/`, and its composition root, `backend/vm-ops-api/rs/src/main.rs`, also serves `--healthcheck` and the three helper subcommands that its sibling containers run. The generated host scripts (the self-redeploy smoke test and swap, the app redeploy) are render functions in the `scripts/` folder of the core's `application/` ring (`backend/vm-ops-api/core/src/application/`), held to their snapshots (`core/tests/snapshots/`) by golden tests. It is the only vm-ops-api, with no standby and no fallback (a broken one is fixed or the VM is recreated): `docs/plans/vm-ops-api-rs/plan.md`, sections 4 and 5.
- Today: `pnpm run arch:services` exits 0. Cargo keeps axum, reqwest and the `rs` crate out of `core`. remote-chat-agent's `rs` crate holds `api`, `infrastructure` and `main.rs` only (ls-lint and `rca-service-holds-adapters-only` keep `domain` and `application` from coming back): its use cases, rules and the ports they call out through are `backend/remote-chat-agent/service-core` (the environment is one of them: `Environment`, with `ProcessEnv` as its adapter), the thread engine is `core`, and the executor WebSocket and the request auth are `infrastructure/ws_channel.rs` and `api/auth.rs`. The only api module that names infrastructure is `api/app.rs`, the composition root (the one ignore of `api-does-not-use-infrastructure.yml`).
- Holes today: the layer rules read `crate::x`, `super::x` and `super::super::x` paths and use lists one level in (`use crate::{api, infrastructure::log}`; the util rules `names-infrastructure`, `names-api` and `names-application` in `backend/arch-utils`), and the purity rule (`does-io`) reads `std`/`tokio` `fs net process env signal`, `reqwest`, `axum` and `dirs` the same way for every crate its `files` globs name. `wf check --budget` holds those crates' manifests to the same rule (`backend-core-io-violations`) and the crates against each other (`cargo-dep-violations`: `exec-testkit` is a dev-dependency of `rca-service`, `rca-exec`, `rca-cli-driver` and `t3-serve` only, an adapter library does not dev-depend on a crate that consumes it, and a crate of the purity registry depends on no adapter library). Only a count catches a core that probes the file system through `Path::exists`, or that calls the system clock or randomness directly (no rule forbids it yet): remote-chat-agent's thread core asks the `Clock` and `IdSource` ports (`backend/remote-chat-agent/core/src/domain/ids.rs`), which the service implements over the system (`rs/src/infrastructure/system.rs`) and the core's tests double (`core/src/testing.rs`, feature `testing`), but `service-core` still makes UUIDs and reads the clock directly (measured 13 Nov 2026: `domain/util.rs` has `uuid_v4`, `random_hex` and `now_ms`, `runs.rs` calls `uuid_v4` twice, and `Instant::now` or `SystemTime::now` appear 15 times in `application/`: `edit_stats`, `attachments`, `one_shot`, `voice_planner`, `log`, `agent_groups/activity`). The budget `core-ambient-calls` (`core_purity::ambient_calls`, limit 17, only shrinks) counts `SystemTime::now`, `Instant::now`, `Uuid::new_v4` and `.exists()` in `service-core`'s sources (not `testing.rs`); the core-purity budget itself reads manifests only, and `service-core` is already in its registry through the `rca-core-crate-does-no-io` globs, so it is held to no I/O crate and no I/O tokio feature. Adapter crates (`opencode`, `speech`, `host`) are held by the ast-grep rule `rca-adapter-crate-names-no-adapter-or-api`: they name no other adapter crate and not `rca-service`.
- Target: the rules match the layer name anywhere in a use path; new rules forbid I/O in the inner rings and in `api`; ignores name files only; each rule has a planted-violation test; `arch:services` runs from each service package's `lint`. History: `docs/plans/rust-services/standard-crates.md:30-39`.

### 2. TypeScript service

Empty since vm-ops-api moved to Rust (`docs/plans/vm-ops-api-rs/plan.md`). The number stays so the others keep theirs.

### 3. Hexagonal CLI

- Decision (23 Oct 2026, `docs/plans/wf-crate-split/plan.md`): wf is split along its seams into `wf-kit` (option parsing, the help engine, the one dispatcher, `wf_err!`, the repo root, the clock), `secrets`, `wf-bucket`, `pipeline`, `wf-vmops`, `pipeline`, `deploy`, `offload`, `deploy`, `wf-harness` (the two device nouns together, over `harness-core`, `harness-browser` and `harness-android`, split on 25 Oct 2026), `wf-bench` and `wf-login`, with `wf` (`routes.rs`, the usage row, dead-refs, knowledge, the e2e runners) as the composition root and `wf-testkit` as the shared test kit. This replaces the `wf-core`/adapters target of `docs/plans/hexagonal-architecture/spec.md` (W1-W9 and open question 12). Cargo's dependency graph, which Nx follows, is the layer wall: a crate depends only on the crates below it, so an edit reruns the crates built on it and no others.
- Rule: a crate owns its ports and ships their doubles behind its feature `testing`; there are no `-mocks` or `-impl` siblings. Each lib has `[lib] test = false` and `doctest = false` and a thin test binary `src/bin/wf-<name>.rs` (about 6 lines over the one dispatcher, mounting only the commands its tests spawn); the crate's `tests/flow.rs` drives that binary through argv. `WF_BIN=<full wf> cargo test -p wf-<name> --test flow` is the parity oracle: the full `wf` serves every thin binary's argv, so a mounting that drifts from `routes.rs` fails it.
- CLI variant rule: a capability gets a port only where a crate's rules hold a decision worth a test (ship, test, check, device, ask/plan, secret). A thin command has no port: its handler calls the adapter, and the crate's flow test covers it.
- Today: no layer rule covers the modules inside a crate. wf is 13 crates and about 36k lines. Two port traits exist (`Device`, `clis/harness-core/src/device/mod.rs`, implemented by `harness-browser` and `harness-android` through `Platform`; the secrets `Backend`, `clis/secrets/src/secrets/backend.rs:111`). `wf check --budget` caps `wf-kit` at 1,200 lines (`wf-kit-lines`): every crate depends on it, so any addition reruns every flow test and needs three dependants.
- Target checks: ast-grep rules for `clis/` (a crate's pure modules do no I/O; a crate names no crate above it) and a `wf check --budget` entry that counts the I/O sites (`Command::new`, `std::fs`, `std::env`, `std::net`, `cli_http`) outside the modules that own them.

### 4. Small tool

- `src/core/` holds the rules (domain and application merged) and takes its ports as parameters (`decide(payload, { rules, condition, approval, repoRelative })`). It imports only the core, `node:path`, `node:url` and `node:crypto`; never another node built-in (`node:fs` or bare `fs`), React, or the `process`/`window`/`document`/`location` globals, directly or through `globalThis`. `src/adapters/` implements the parameters. `src/ui/` exists only for a page. Entry files at the `src/` root are the composition roots. Rule: entry → {ui, adapters} → core.
- Rust variant (agent-config-sync): pure modules never import the shell (`fsx`, `sync`, `cli`); a temp checkout is the double.
- Today: the seams exist (`libraries/hook-runtime/src/dispatch.mjs` takes `run`), but I/O sits inside the guard module (`libraries/hook-runtime/src/guard.mjs`), and the browser harness is six packages, one per layer plus the page, whose walls `shamefullyHoist` (`pnpm-workspace.yaml:120`) makes porous. No check.
- Target checks: an oxlint override for `src/core/` and `src/ui/` run by each tool's `lint` (bare built-ins such as `fs` and `globalThis.process` included), ls-lint whitelists.

### 5. Core library

- Pure modules, port traits, fakes behind a feature. An adapter that several consumers share is its own adapter crate (type 6), not a feature of the core.
- Today: rca-core's port is `RunHost` (`libraries/rca-core/plugins/ladder/src/run.rs:49`), but its file store calls `std::fs` (`libraries/rca-core/src/file_store.rs:19-54`). Target: the file store moves into an adapter crate used by wf's adapters and rca-service's infrastructure; the core-does-no-io rule guards rca-core.

### 6. Adapter library

- Flat modules over one outside system; no product rules. The consumer defines its own port over it. cli-http gets no `Transport` trait until a named flow test needs one: wf's use cases are tested over port fakes, and its adapters against loopback fakes (`clis/wf-vmops/src/testing.rs`).
- Tests are integration tests against a fake of the outside system (cdp: a fake Chromium; `@lib/browser-client`: a fake `WF_BIN` and a node:http server).

### 7. App shell

- `app/`: expo-router. `_layout.tsx` files compose providers, install the logger and crash handler, and wire library ports to shell adapters. Every other route file is a one-line re-export of a feature's `routes/*` module or an `app-shell/` screen. `app-shell/`: shell UI. `lib/`: shell adapters and platform shims, imported only by `app/` and `app-shell/`. The shell reaches a feature only through its barrel, `routes/*` or `testing`; nothing under `features/` imports the shell.
- Today: feature routes are one-line re-exports, but `app/(app)/(content)/showcase.tsx` is a 298-line screen and dashboards lives in `components/`. There is no `hooks/`, `design-system/` or `config/` directory; the design system is `@lib/tamagui-design-system`. Nothing checks the shell: the app's `lint` is oxlint only (`frontend/mla-tamagui/package.json:12`), and the thin-route lint rule was removed (`docs/plans/after-device-lab/spec.md:42`).
- Target check: a dependency-cruiser app-shell preset in the app's `lint`.

### 8. Hexagonal feature

Target rings:
- `domain/`: entities, zod schemas, pure rules, bundled catalogues, and `domain/ports.ts` with one `<Feature>Ports` object. Imports only `domain/`, zod, contracts packages, `@lib/voice-edit-dsl` and `@lib/logging`.
- `data/`: adapters, each a factory `create<Tech><Port>(config, io?)` with an injectable `fetch`, `WebSocket` or storage. Imports `domain/` only. Network globals appear nowhere else.
- `state/`: use-case hooks and stores, plus `state/ports.tsx` (the context and `use<Feature>Ports()`, which throws outside its provider). Imports `domain/` only. A store may persist through zustand `persist` and AsyncStorage, which the flow harness stubs (`frontend/libraries/flow-test-harness/src/vitest/flowTestConfig.ts:67-68`).
- `ui/`: imports `state/` and `domain/`. A native view (WebView, Skia) is ui. Device services (clipboard, Linking, expo-updates) are called where used. The harness has no stub for them: in a flow test they run through react-native-web or the module's own web build. One becomes a port only when a test must drive or observe it.
- `di/`: the composition root, the only reader of `@lib/app-di`. Builds adapters from `@lib/app-config` values, merges overrides typed `Partial<<Feature>Ports>`, provides them through `state/ports.tsx`.
- `routes/`: the modules `app/` re-exports; `routes/_layout.tsx` mounts `di/` once. Imports `di/` and `ui/` only. A route whose screen reads `use<Feature>Ports()` sits under the one `app/` subtree whose `_layout.tsx` re-exports that layout; a shell flow test renders every feature route and fails on a missing provider.
- `testing/`: doubles and e2e fake servers; no production ring imports it. A fixture mode that ships in the product (leetcode's `EXPO_PUBLIC_LEETCODE_FIXTURES`, `di/fixtureOverrides.tsx:4-8`) stays in `data/`, and `testing/` re-exports it. `index.ts`: the barrel.
- Rule: routes → di → data → domain; routes → ui → state → domain; di → state. The public surface is the barrel, `routes/*` and `testing`. A feature imports no other feature and nothing from the shell. Old names map to rings: `contracts/`, `use-cases/`, `lib/` → `domain/`; `hooks/`, `integration/` → `state/`; `fixtures/` → `testing/`.

Today:
- Every feature but dashboards sits in `frontend/mla-tamagui/features/<name>`: in-app Nx projects (leetcode, tamil-writing) or workspace packages.
- The four package features check layers with the shared preset `frontend/feature-kit/depcruise.feature.cjs:3-8`: `domain` imports no layer; `data` imports `domain`; `state` imports `domain`, `data`; `ui` imports `domain`, `data`, `state`; files outside a layer are free. All four pass (21 Oct). coding-agent adds `integration` and counts `contracts/` and `fixtures/` as domain; service-report counts `contracts/` as domain. That chain puts the adapters (`data/`) under the application (`state/`, `integration/`); the target replaces it.
- leetcode's own rules (`frontend/mla-tamagui/.dependency-cruiser.cjs`) forbid `ui → data` except two files and run only by hand (`pnpm --filter mla-tamagui run arch`, 323 modules, 0 violations on 21 Oct). tamil-writing and dashboards have no check.
- leetcode is the composition reference: `LeetCodePorts` (`features/leetcode/domain/ports.ts:143`) is built once in `frontend/mla-tamagui/features/leetcode/di/LeetCodeContainer.tsx` (:12, :51-59), mounted once in `routes/_layout.tsx`, read with `useLeetCodePorts()`, which throws outside the provider (:42-48). It has no `domain/` yet.
- The test seam is `useDependencyOverrides(key)` (`frontend/libraries/app-di/src/index.tsx:31-36`); keys and shapes are in `docs/solutions/testing/flow-tests-mount-whole-routes.md`. Outside leetcode it is read in ui, state and integration files (coding-agent `ui/ThreadListScreen.tsx:102`, `ui/DispatchRunScreen.tsx:56`, `integration/useGateway.ts:25`; service-report `state/useHealthReportHistory.ts:33`; dashboards `AgentsDashboardScreen.tsx:226`, `InfraDashboardScreen.tsx:229`) and in dev-tools' dead `di/useDevToolsRuntime.ts:7`, never in a live composition root.
- `ui` imports `data/` in coding-agent (4 files), tamil-writing (4) and leetcode (2, exempt).

Target checks: a hexagonal-feature dependency-cruiser preset (every module sits in a ring, the ring rules, only `di/` composes, feature is a leaf, `no-circular` as error) in each feature's `lint`, the app's `lint` cruising the in-app features in place of the `arch` script, and an oxlint ban on network globals outside `data/`.

### 9. Presentation feature

`ui/`, `routes/` and optional local `state/`; no domain, data, di or ports. It becomes type 8 with its first backend call, persisted store or business rule. Today settings fits and passes the shared preset. dev-tools is type 9 only until the open question on dissolving it into the app shell is answered.

### 10. Contracts package

A flat `src/` of zod schemas and inferred types; zod is the only dependency; no React, React Native, node built-ins or I/O. It exists only while two or more packages consume it; otherwise its schemas belong in the consumer's `domain/`. A wire contract that a Rust service mirrors carries golden JSON fixtures that zod and serde both parse (today `backend/remote-chat-agent/core/src/domain/types.rs` mirrors `@app/remote-chat-agent-contracts` with no check).

### 11. Pure library

A flat `src/` with a barrel; no ports, adapters or DI; imports only relative modules and an allow-list of pure dependencies (zod, estree types, react for `@lib/app-di`). No node built-ins, React Native, Expo, globals or workspace packages. Unit tests only. `@lib/app-config` joins as a pure `resolveAppConfig({ env, ... })`; the literal `process.env.EXPO_PUBLIC_X` reads, which Expo inlines only when written out, move to the app's composition root.

### 12. Hexagonal library

- Core (types, rules, functions or hooks taking ports as parameters), ports named after the need (`LogPersistence`, `DocumentTreeDriver`, `KeyValueStorage`; no port for a device service with one call site, such as rn-agentation's clipboard), adapters (the only files that import Expo modules or AsyncStorage, or touch `fetch`, `window` or `process.env`; platform forks live here), testing exported as `./testing`. The host app is the composition root. Existing folder names map to roles in each library's dependency-cruiser config.
- Between packages: no library imports `frontend/mla-tamagui`; a hexagonal library may import `@lib/logging` only.
- Today: `@lib/logging` and `@lib/rn-agentation` already have ports; no library has a layer check, and six frontend libraries have no `lint` script.

### 13. Component library

`theme/` → `primitives.tsx` and `icons.ts` → `ui/` → `composites/`; `lib/` is a leaf. `primitives.tsx` is the single door to tamagui and `icons.ts` the single door to lucide (`frontend/.oxlintrc.json:62-71`). Not hexagonal: the components are the delivery. Its two I/O touches are chosen by file fork, with no port: key-value storage (`lib/keyValueStore.ts`, plus a `.native.ts` over AsyncStorage as target, because the persisted stores hydrate at import, before the host could inject anything) and the keyboard source.

### 14. Test harness

A driver API, native-module stubs, and the vitest host wiring as composition root. It stays feature-agnostic: it never ships a feature's doubles and never imports app or feature code. Today `@lib/flow-test-harness` still exports leetcode's audio and voice fakes.

### 15. Glue and 16. Config and data

No rings. Glue is flat, one file per entry point, and holds no decision; a decision moves into wf (root rule: capability goes in a `clis/*` package) or into a small tool. Config is guarded by format checks and static contract tests owned by a package.

## Where each rule runs today

| Rule | Tool and config | Command | Pre-commit | CI pipeline |
| --- | --- | --- | --- | --- |
| Rust layer direction | ast-grep, `backend/arch-rules/` | `pnpm run arch:services` | when a matching file is staged (`lefthook.yml:41-44`) | no |
| Rust folder layout | ls-lint, `.ls-lint.yml` | `pnpm run arch:services` | same | no |
| Rust `core` cannot use `rs`, axum, reqwest | Cargo | any build | compile | yes |
| Rust `core` names no I/O crate and builds tokio without `fs`, `net`, `process`, `signal`, `io-util` (the workspace tokio carries none either) | manifests read by `clis/pipeline/src/core_purity.rs`, the `backend-core-io-violations` budget | `wf check --budget` | every commit | yes, with the budget in `wf check --staged` |
| Package-feature layers | dependency-cruiser, `frontend/feature-kit/depcruise.feature.cjs` | each package's `lint` | no (gap below) | yes, typecheck-lint-unit-tests (`clis/pipeline/src/test_plan.rs:19`) |
| leetcode layers | dependency-cruiser, `frontend/mla-tamagui/.dependency-cruiser.cjs` | `pnpm --filter mla-tamagui run arch` | no | no |
| No `vi.mock` in flow tests | oxlint, `frontend/.oxlintrc.json:125-135` | the package's `lint` | where that lint runs | yes |

Nothing checks: tamil-writing, dashboards, the app shell, feature entry points and cross-feature imports, `frontend/libraries`, `libraries/`, `tooling/` or `clis/`. The architecture workflow (`.github/workflows/architecture.yml`) runs the `arch` and `arch:services` scripts too, but only on `workflow_dispatch` (:16-17).

Pre-commit gap: `wf check --staged` runs the `lint` of the package that owns each staged file, found through a fixed list of parent directories (`clis/pipeline/src/verify.rs:285`). The list has no `frontend/mla-tamagui/features` or `frontend/libraries` entry, so a feature-package file falls to the app's `lint`, which skips the package directories (`frontend/mla-tamagui/package.json:12`), and a `frontend/libraries` file falls to the root `lint`, which skips `frontend/**` (`package.json:14`). It also runs `lint` only for staged JavaScript and TypeScript files (`verify.rs:167-169,364-365`): a staged `.rs` or `.yml` file reaches no package `lint`, only lefthook's `service-architecture` glob job. Target: owners come from the pnpm workspace listing `clis/pipeline/src/verify.rs` already reads for typecheck and tests.

- **No new dependency edge from a low-level crate to a heavy one.** Every `clis` crate has a blast radius: the crates rebuilt and retested when it changes (its dependants through normal, build and dev edges) and the test stages they reach. Low-level means a radius of at least a quarter of the crates (`wf-kit` and the other bottom crates), heavy means a dependency closure of at least a quarter; both are read from the manifests, never listed by hand. A radius stays within its budget in `clis/pipeline/data/blast-radius.json` (today's measured values: it only shrinks, and raising one needs the owner's reason in the commit message), and an existing low-level to heavy edge stays only with a reason under `allowed_edges`. `wf check --blast-radius` prints the radii as JSON and fails with the shortest dependency path that pulls a crate in; `wf check --budget` runs it as `blast-radius-violations`, and `wf check --unused-deps` lists declared-but-unused dependencies.

## How to add a rule

- Put it in the owning package's `lint`, so CI's typecheck-lint-unit-tests stage runs it. No architecture rule lives only in a manual workflow.
- Make it reach pre-commit and the cache. `wf check --staged` runs a `lint` only when a staged file is JavaScript or TypeScript (`clis/pipeline/src/verify.rs:167-169,364-365`), so a rule over `.rs` or `.yml` files also needs its source, rule and test paths in the `service-architecture` glob (`lefthook.yml:43`). Its rule and test files must be inputs of that `lint` in `nx.json` (the `sharedGlobals` at `nx.json:7-17` hold none today), or a rule-only change replays a cached pass.
- Land it green with a ratchet: today's violations as named files (ast-grep `ignores`, or a dependency-cruiser known-violations baseline read with `--ignore-known`). Each migration unit deletes its own entries.
- Prove it fires: a planted-violation test with one bad and one good example per rule (`ast-grep test` over rule tests; dependency-cruiser's `cruise()` over planted fixtures). `ast-grep test` passes with zero tests, so the check also asserts `N passed` with N equal to the rule count.
- No Nx module-boundary rule. Nx stays for task caching (`nx.json`); its tags carry no rule.

## Tests per ring

Test types are those of `docs/solutions/testing/verification-ladder.md`.

| Ring | Test type | Doubled | Example |
| --- | --- | --- | --- |
| domain | unit | nothing | tamil-writing's glyph rules; `#[cfg(test)]` beside a Rust domain function |
| application | unit | the package's own port fakes | a leetcode-backend use case over `FakeLeetcode` and `FixedClock` (`backend/leetcode-backend/core/src/testing.rs`) |
| driven adapter | flow when the transport is an object passed in; integration when it is a loopback server or a stub binary on `PATH` (`verification-ladder.md:44`); a contract test when one generic test runs against the fake and the real adapter | the transport: an injected `fetchImpl` or storage stub, a temp dir, a loopback server, a stub binary | `@lib/browser-client` against a fake `WF_BIN` and an HTTP server (integration) |
| composition root and driving adapters | flow | ports only, plus native modules the harness stubs | a route through `createFlowApp` with `overrides`; a service's `api::app` driven with `oneshot` over core fakes |
| the whole system | deployment verification, browser or Android e2e, runbook | nothing | `test:verify:deployed`; `e2e/editor-typing.lua` |

- A unit test never mounts a route, starts a server or binary, or spawns a process.
- A flow test boots the real composition root; its only injection is at ports. Frontend: `overrides: { '<key>': Partial<Ports> }` read by the composition root. Rust: the assembly function `main` calls, with core fakes and config passed as a struct. wf: each crate's flow tests spawn its thin binary through argv (the crate-split decision D1; the in-process `run(argv, world, io)` of the older spec is dropped), with the outside world doubled by `wf-testkit` and by the doubles of the crate that owns the port. Target for small tools: the entry function with fake adapter parameters.
- Doubles live with the port's owner, never in a production ring and never as an env switch in production code.
- No `vi.mock` of a project module in a flow test (oxlint rejects it, `frontend/.oxlintrc.json:126-134`). The target adds oxlint bans on `vi.spyOn` in flow tests and on `vi.mock` in library tests; neither is checked today.

## Retired

- **The "Clean architecture standard"** (`docs/archive/README.md` and its summary `clean-architecture-standard.md` beside it). Its five roles `contracts ← data ← use-cases ← delivery ← di` and "contracts, not domain" gave way to domain/application/adapters, which the Rust services and features already use. Kept from it: factory functions instead of classes, and the composition root that builds the graph while the entry point runs it.
- **Two-tier DI** (`dataContainer` plus `useCasesContainer`): built in the TypeScript packages (`src/di/useCasesContainer.ts` in leetcode-backend, vm-gateway, agent-config-sync, env-bootstrap, infra-ops, secrets and vnc-auth-capture) and removed with that TypeScript between 4 and 9 Oct (for example `af6d6b504`, `9b03b119d`, `a0595f138`). No current package uses it. leetcode has one container plus the override seam; the target gives every hexagonal feature the same.
- **Nx module boundaries** (`enforce-module-boundaries`, `scope:*` and `layer:*` tags): enforced at `error` through ESLint and `nx.json` `depConstraints` until `923d53408` (4 Oct) removed both; no such rule exists today. Rules live in dependency-cruiser, ast-grep, ls-lint, oxlint and Cargo.
- **Package per layer**: pnpm hoisting makes the walls porous; a TypeScript layer is a folder checked by lint.
- **The Feature Triad** (contracts, `-impl` and `-mocks` siblings): a feature is one package whose doubles ship from its `testing` entry.
- **convention-lint and the dropped lint rules** (`root.test-tier-matches-layer`, `root.test-smells-clean`, `no-direct-io-in-feature-ui`, `thin-route`): the tooling is gone; their intent is in the target checks above.
- **"The slim wf: hexagonal, one esbuild bundle"** (`docs/plans/greenfield/requirements.md:269`): wf is a set of Rust crates (`clis/wf-*`); its target is type 3.
- **The five package categories** (`ui-feature`, `backend-service`, `cli-tool`, `eslint-plugin`, `pure-logic`, goal `docs/goals/2026-08-22-coverage-debt-and-browser-consolidation.md`): superseded by the 16 package types.
- **The other files in `docs/archive/README.md`** are history; where one disagrees with this doc, this doc wins. Three hold concepts this list retires or keeps: `two-tier-di-containers.md` and `feature-triad.md` (retired above) and `di-builds-graph-app-boots-it.md` (kept: the composition root builds the graph, the entry point runs it). `cycle-avoidance-patterns.md` and `splitting-writes-not-state.md` still teach the retired feature chain with `data/` below `state/`.
