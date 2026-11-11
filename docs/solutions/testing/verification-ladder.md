---
title: Test types — what each covers, what it doubles, one example each
date: 2026-09-05
updated: 2026-10-24
category: testing
tags: [testing, vitest, e2e, runbooks, visual, verification]
problem_type: knowledge
module: docs
---

Every test in this repo is one of these types, and a report names the type that ran. The four in the middle were agreed with the owner on 18 Oct 2026 and replace the older ladder (`unit`, `contract`, `flow`, `integration`, `e2e`, `visual`); integration and runbook keep their earlier definitions.

## The types

| Type | What it covers | Doubled | Example |
| --- | --- | --- | --- |
| Unit | One piece of business logic (a domain rule or a use case), no I/O | Every port it uses | wf's free-tier check: an A1 VM at 2 OCPU / 12 GB passes, 3 OCPU is refused (`clis/deploy/src/infra.rs`, `free_tier`) |
| Flow | The whole service or app, in-process, through its main entry point: the router, `main`, the CLI binary, a mounted route | Only the outside world: network, vault, LeetCode, opencode, adb | `backend/leetcode-backend/rs/tests/flow.rs`: the real router with its auth and rate limit, a submit journey, LeetCode doubled |
| Integration | The real service, started as it runs in production (its binary or process) | The traffic to the services around it, mocked: fake servers, stub binaries on `PATH`, mock upstreams | `backend/vm-ops-api/integration/`: the real vm-ops-api process over real HTTP, with its Docker Engine and its `docker` and `wf` binaries doubled (a fake Engine on a Unix socket and stubs on `PATH`) |
| Deployment verification | A deployed service on the VM, with its real dependencies, through its public interface | Nothing | remote-chat-agent's `test:verify:deployed`: POST /api/chat to the deployed agent, expect an SSE stream with no error |
| Browser / Android e2e | The real app, driven like a user: a web export of the tree that the run serves on loopback (`wf test prepare --web`) for Chromium, the EAS Update CI published for the commit (`against: update`) on `rdl`, the one Android device | Nothing (a network stub only where the test says so) | `e2e/editor-typing.lua`: open two-sum, type with the keyboard, tap Done, Run Local, check the result |
| Runbook | A plain-text, step-by-step test written as Given/When/Then, executed entirely by an agent against the deployed system and recorded (`wf offload ask --runbook <name>`) | Nothing | `docs/runbooks/device-lab-codemirror-typing.md`: the CodeMirror typing gate, every step run and checked by an agent |

Order of importance and of reporting, top-down: runbook, browser / Android e2e, deployment verification, integration, flow, unit.

### What does not belong where

- A flow test never stops at a layer: it enters where a user or caller enters and goes all the way through. Narrowing it to the application layer turns it into a unit test with extra setup (tried and reverted on 18 Oct).
- A unit test never starts the router, a server, a binary or a device; if it needs one, it is a flow test.
- An e2e or runbook check doubles nothing it can reach for real; a stub it needs is named in the test.

## Flow, integration and e2e: the three that get confused

All three go through a real entry point. They differ in where the doubles sit and whether anything real is on the other side.

| | flow | integration | e2e |
| --- | --- | --- | --- |
| entry point | called in-process (router, `run(args)`) | the real binary or process, a black box | the deployed service, a black box |
| external dependencies | replaced in code (a fake injected through a port) | replaced over the network or process boundary (fake server, stub binary) | real: real vault, real services, real network |
| spawns a process or opens a socket | no | yes | yes, on the VM |
| how it reads | a unit test whose subject is a big piece of code | a user driving the program against a staged world | a user driving production |
| fails when | wiring or logic is wrong | the built artifact, its config or its wire protocol is wrong | the real environment disagrees with our assumptions |

Rule of thumb: if the test replaces a dependency by passing a different object, it is a flow test; if it replaces it by running something else on a port or on `PATH`, it is an integration test; if nothing is replaced, it is e2e.

Worked example, `backend/remote-chat-agent` (Rust):
- flow: the axum `Router` driven with `tower::ServiceExt::oneshot`, the agent backend, event store and clock replaced by fakes in `src/testing/`. No socket, no spawned process.
- integration: `tests/blackbox` starts the real `rca-service` binary, points it at `fakeOpencode.ts` (a fake server) and talks to it over real HTTP and WebSocket.
- e2e: `wf vm exec` probes the deployed `remote-chat-agent` on the VM with the real vault and the real opencode.

Naming trap in Rust: Cargo calls every file under `tests/` an "integration test", but those run in-process against the crate's public API, which is a flow test here. Name them `tests/flow*.rs`, never `tests/integration*.rs`; integration tests in this repo are the black-box suites started as real processes; a package runs its suite as its `test:integration` script. A new integration suite is written in Rust: a crate of its own whose tests start the service's process, or, in a `clis/wf-*` crate, a `tests/integration.rs` for the journeys that need a real process the flow tests double (a Chromium, ffmpeg): its `cargo-integration` Nx task runs one test at a time. CI runs every integration test in the `deployment-verification-tests` stage, after the CLIs are built and the services deployed, never in `flow-tests`. A real Chromium in a flow test is a violation of the flow rule: move the test.

## Test behavior, not the implementation

A test says what happens, not what is written. Not "the registry has a `check` for this key", not "the SQL contains `MERGE INTO`", not "the leg for `reporting-service` ships when `libsqlite3-sys` moves": each of those fails when the code is restructured and still passes when the feature is broken. Instead:

- assert the outcome of using the thing (the rows read back, the leg count a moved crate produces, the refusal a missing key causes);
- derive inputs from the data under test: read `groups.json`, the lockfile or the registry in the test and compute the expectation, so adding a key or dropping a dependency does not break a test that is about something else;
- if the only thing left to assert is the text of a constant, a query or a wiring line, do not write the test; verify it one tier up (a live run against the real dependency, or a flow test through the entry point) and say so;
- a data-completeness lint (every key has a check) is a policy, not a test: make the behavior it protects observable (`wf secret verify` reports an unchecked key) and test that.

## Placement rules

- File suffix encodes the tier: `*.unit.test.ts`, `*.contract.test.ts`, `*.flow.test.ts`, `*.integration.test.ts`, `*.e2e.test.ts`. Directory `tests/<tier>/` where a package uses `tests/`, `src/**/__tests__/*.<tier>.test.ts(x)` where it co-locates. Legacy names are grandfathered, never renamed by this rule.
- Rust crates: flow tests are `tests/flow*.rs` (or `#[cfg(test)]` modules that drive the router or `run`), contract tests share one generic function across an adapter and its fake, and the integration tier is the package's `test:integration` script or a crate's `tests/integration.rs`, which CI's `integration-tests` stage (a crate's real-dependency `tests/e2e.rs` is the `cli-e2e-tests` stage) runs through the Nx cache; a new suite is written in Rust.
- A `clis/wf-*` crate's flow tests enter through the crate's thin binary `src/bin/wf-<name>.rs` (cargo's `CARGO_BIN_EXE_wf-<name>`, which `wf_testkit::wf_binary()` reads), with the outside world doubled by `wf-testkit` (stub tools first on `PATH`, mock servers) and the doubles of the crate that owns each port (`wf-vmops`, `wf-harness`, feature `testing`). `WF_BIN=<full wf> cargo test -p wf-<name> --test flow` runs the same suite through the real router (the parity oracle). What stays in `clis/wf/tests` is the thin integration tier: help surface, dead-refs, usage row, mcp-guard, knowledge, log emit, and the e2e runners.
- `visual` lives in runbooks and units (`judge` + `judgeInput` png), never in vitest.
- A unit test covers a domain rule or a use case with its ports doubled; it never mounts a route, starts a server or binary, or mocks its own module. No lint checks this.
- A flow test boots the real composition root (a service's router assembly, a feature's route layout through `createFlowApp`) with doubles only at ports (plus the harness's native-module stubs where jsdom has no runtime), and asserts on observable output; it never reaches into internals. Which ring each test type covers: `docs/solutions/architecture/hexagonal-architecture-by-package-type.md`.
- Given/When/Then everywhere; the tier name appears in `given`.

## `units.jsonl` `tier` → gate

A unit's `tier` is REQUIRED (`TIERS` in `clis/offload/src/worker_plan.rs` = `unit, contract, flow, integration, e2e, visual`); a missing or unknown tier fails the plan. The per-tier gate templates the planner fills:

- `unit` / `contract` / `flow` → vitest glob. `e2e` / `visual` never carry a vitest gate.
- `integration` → the package's `test:integration` script, scoped to the test file; CI's `integration-tests` stage runs the whole script through the Nx cache, after the deploy, and a new suite is written in Rust.
- `e2e` → a runbook step (`wf offload ask --runbook <name>`) or a `wf browser` gate against the VM.
- `visual` → `judge` (a vision-capable model) plus a `judgeInput` ending in `.png|.jpg|.jpeg|.webp`; both required.

`buildUnitPrompt` renders `TIER: <tier> -- <guidance>` right after `CONSTRAINTS`; `formatCheck` prints `[<tier>]` next to each id; `formatManifest` adds a `tiers: unit N · flow M · e2e K` histogram line after the header (only tiers with count > 0, in enum order).

## Reporting order

Top-down: runbook, browser / Android e2e, deployment verification, integration, flow, unit (the plan's tier enum still names `contract` and `visual`; a contract test reports as unit, a visual check as part of its e2e or runbook). A report names the highest tier that actually ran and states plainly which tiers did not. "I ran the unit tests" is not verification when an upper tier is reachable — see `test-priority-runbook-and-e2e-first.md`. When an upper tier is blocked, fix the blocker or file it (`docs/known-gaps.md`); do not silently fall back to the tier below and present it as verification.

## Where each tier runs

Every tier but a browser e2e test runs on the VM in a session worktree (`/workspace/sessions/<key>`), by owner direction, because the VM is the reference environment. The sandbox lints, orchestrates via `wf vm` and `wf offload plan`, and runs a browser e2e test against a web export of the tree served on loopback (`wf test prepare --web`, then `wf test run <test> --platform browser`: no dev server, no worktree, no backend baked in). E2E and visual run where the system is: an Android e2e test loads the EAS Update CI published, and a runbook runs through a `wf vm exec` script running `wf offload ask --runbook` (an automated step that drives the real entry point and records screenshots). A local sandbox path remains the documented fallback for e2e and visual when the VM is unreachable, but a broken sandbox must not change a verdict: if the VM run is required by the unit's tier and the sandbox cannot stand in for it, the verdict is the VM's result and the sandbox path is recorded as a fallback attempt, not a substitute.

## See also

- Cheap-flow planner: `.agents/skills/cf-plan/SKILL.md` — how to choose a tier; half-done work gates at `unit`/`flow`, a finished UI unit gates at `visual`.
- Cheap-flow worker: `.agents/skills/cf-work/SKILL.md` — manifest reads the `tiers:` line.
- Evidence handling: `docs/archive/README.md` — evidence referenced again is published with a link, not pasted inline.
- Doctrine source: `docs/archive/README.md`; tier enum: `clis/offload/src/worker_plan.rs`.