# Platform in the cache keys: two tiers, per crate

Date: 2026-11-04. Status: implemented and committed (e78926cb); pushed with the M8+M9 push.

## Question

Should the CPU platform (x86_64 vs arm64) be part of the Nx cache key of a cargo task? A pass on the x86_64 sandbox should replay on the arm64 CI VM, and the other way round, unless the code under test can behave differently per CPU.

## What was audited

Two read-only audits preceded the decision. The raw NXHASH tables of the audits were lost in a container restart; the figures below are the audits' findings as handed to this unit, and were not recomputed (only the "verified" rows were re-checked in the tree).

| Subject | Finding |
| --- | --- |
| Crates | 68 audited |
| `cfg` hits on CPU properties | 15 hits; every one was a pointer to code the CPU does not change, except `clis/offload/src/worker_run.rs` (`cfg!(target_arch)` default of an `arm64` capability) |
| Native tools | none whose output differs per CPU in a way a test asserts |
| Hash inputs | the platform entered only through the `native` input |
| Sandbox vs CI | task hashes compared with `wf test drift` tables (NXHASH) |

## Verified facts

- No platform is in any default cache key (`nx.json`, `tooling/nx/cargo-crates.js`).
- TypeScript task hashes were identical across x86_64 and arm64.
- The Rust hash differences between the two were file inputs only (paths, generated files), not the platform.
- The earlier CI cache misses had other causes: the first pushes after the key change, and gate runs under an unpinned `node`.

## Decision (owner policy)

- Two tiers, decided per crate.
- Default tier: no platform in the key.
- Platform tier: the named input `platform` (`node -p "process.platform+'-'+process.arch"`) is added to every cargo task of the crate.
- Auto-detect: the crate's own code uses `cfg!`, `#[cfg]`, `#![cfg]` or `cfg_attr` on `target_arch`, `target_pointer_width` or `target_endian` (comments and strings ignored).
- Opt-in: `[package.metadata.flow] platform = true`. Opt-out of a scan hit: `platform = false`, with the reason in a comment.
- No inheritance: a dependent of a platform crate stays in the default tier.

## What changed

| File | Change |
| --- | --- |
| `nx.json` | named input `platform` |
| `tooling/nx/cargo-crates.js` | manifest flag, comment/string-aware scan, `platformTier()`, `platform` input on a platform-tier crate's tasks and on the opted-in package suites that run one |
| `clis/pipeline/src/nx_drift.rs` | `platform` is a drift cause kind, with a unit test |
| `clis/pipeline/tests/flow/nx_cache.rs` | platform double feeds the real one-liner (`linux-x64`, `linux-arm64`); default tier gives the same key on both |
| `clis/pipeline/tests/flow/nx_crates.rs` | `tiers` fixture workspace and the planted-change test |
| `clis/wf/src/help/wf__test.txt`, `docs/reference/wf-cli.md` | tier text, `platform` cause kind |
| `docs/solutions/ci/cache-layers.md` | section "Platform tiers" |
| `clis/telemetry/src/source.rs`, `clis/wf/src/usage_row.rs` | `std::env::consts::OS` instead of a `cfg!` OS table |
| `clis/offload/src/worker_plan.rs`, `worker_run.rs`, `tests/flow/worker.rs` | the `arm64` worker capability and `needs: arm64` removed (`needs` is `any\|deployed`) |
| `.agents/skills/cf-plan/SKILL.md`, `.claude/skills/cf-plan/SKILL.md` | `needs` is `any\|deployed` (the `.claude` copy is generated; edited by hand, run `pnpm run sync:agent-config` to confirm no drift) |
| `docs/plans/greenfield/contract.md`, `requirements.md` | `arm64` dropped from the `needs` text |

Planted-change test (`only_a_crate_in_the_platform_tier_runs_again_on_another_platform_and_its_dependents_do_not`): moving from x86_64 to arm64 reruns only the flagged and the auto-detected crate. Stay on their x86_64 pass: a crate whose hits are only comments, strings, raw strings, `cfg(unix)` or `target_os`; a crate waived with `platform = false`; a dependent of the flagged crate; a plain crate. Adding `cfg!(target_arch)` to a plain crate moves it into the tier; removing it restores its old pass.

## Crates in the platform tier now

None. `clis/offload` was the only one (its `worker_run.rs` hit) and the cleanup removed the hit; `npx nx show project crate-offload --json`, `crate-pipeline` and `crate-telemetry` contain no `platform` input. The only `target_arch`, `target_pointer_width` or `target_endian` text left in the repo is test fixture data in `clis/pipeline/tests/flow/nx_crates.rs`. Nx recomputes the graph only when a manifest changes, so a local graph cached before a `cfg` edit can be stale until a `Cargo.toml` changes; a clean CI checkout has no such cache.

## What stays, and why

| Item | Where | Why it stays |
| --- | --- | --- |
| macOS keychain backend | `clis/secrets` | selected by OS, not CPU; local only, CI uses Bitwarden |
| `ru_maxrss` units | `clis/wf-bench/src/bench.rs` | differ by OS (bytes vs KiB), not CPU |
| `prctl` | `backend/vm-ops-api/integration/src/lib.rs` | Linux call, every cache writer is Linux |
| `cfg(unix)` | `clis/t3/store/src/lib.rs` | `unix` is no CPU property |
| `darwin` mapping | `clis/wf-bench/src/app_bench/export.rs:183` | outside this work |

## Open items

- Chromium pixel goldens allow 0.2% differing pixels: spot-check one golden on arm64 against x86_64.
- Decide whether to go Linux-only for the keychain code.
- `cargo test -p offload` e2e: 3 `tests/e2e.rs::ask` tests failed in the cleanup unit (they drive the real `opencode`); cause not established, not yet shown to be unrelated.
- `cargo test -p telemetry` from the workspace root does not compile its flow test (`callsite` feature); passes with `--all-features` from the crate directory. Whether this predates the change is unchecked.

## How to prove it

1. On the sandbox: `wf test drift --save <table>`.
2. Take the table the CI log prints under `NXHASH` and run `wf test drift --against <that table>`.
3. Expect no `platform` cause and, for the default tier, no differing hash caused by the CPU.
