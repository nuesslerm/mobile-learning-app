# wf lazy build: a launcher that builds a verb's crate on first use

## Problem

The full `wf` links 24 crates (closure of `clis/wf`), so every first build in the sandbox compiles all of them and its test executables fill the disk (see `docs/audits/2026-11-08-gate-dependency-audit.md`). Most sessions use two or three verbs. Each wf concept already ships as its own thin binary (`deploy`, `offload`, `knowledge`, `agent-config-sync`, `wf-lib`, `wf-mimic`, `harness-flow` and the other `src/bin/*.rs` of the crates), and `WF_BIN=<full wf> cargo test -p wf-<name> --test flow` is the parity oracle that checks the thin binary and the router agree.

## Design

A small launcher `wf` (its own crate depending on `wf-kit` only) that holds one table, verb to (package, thin binary), derived at build time from the crates' `routes`, and does this on a call:

1. Look up the verb's first word. An unknown verb, `--help`, `--version` and `help` run the full router (built on first use like any crate), so the help table stays the one in `clis/wf/src/help_table.rs`.
2. Find the thin binary under the target directory (`CARGO_TARGET_DIR`, else `clis/target`) newer than its crate's sources by the fingerprint file cargo writes. If current, `exec` it with the same argv, stdin and environment.
3. If missing or stale, take `flock` on `<target>/.wf-lazy.lock` (the second caller waits and then finds it built), run `cargo build --locked -p <package> --bin <thin>` with `CARGO_INCREMENTAL=0` with its stderr passed through, then `exec`. A failed build exits with cargo's code and prints the command to rerun.
4. Verbs that need two crates (`wf ship` runs `deploy` over `pipeline`) are one thin binary whose closure holds both; the table never names two.

What each verb builds on first use: the crate that owns it and its closure, per `wf check --blast-radius` (`deploy` 15 crates, `offload` 5, `knowledge` 3, `parity` 3, `agent-config-sync` 1, `secrets` 3, `wf-bucket` 7), instead of 24 for any verb. Never built by a verb that does not name it: the orca-serve and t3-serve crates and the harness crates unless a verb of theirs runs.

## How it is tested

Types and what each covers:

- **Flow** (`clis/wf-lazy/tests/flow.rs`, through the launcher binary, `cargo` doubled by a stub that records its argv and writes a fake thin binary): a verb whose thin binary is missing builds exactly its package once and then runs it; a second call runs without calling cargo; touching a source of the crate rebuilds only that crate; two concurrent first calls build once; a failed build exits with cargo's code and no verb runs; an unknown verb runs the full router.
- **Table check** (unit over `cargo metadata`, in `wf check --budget`): every verb of `routes::COMMANDS` has exactly one row, and every row's binary exists in its package; the table is derived, so a new verb with no thin binary fails the pre-commit gate.
- **Parity** (`WF_BIN=<launcher> cargo test -p wf-<name> --test flow` for each crate in the post-land `rust-clis` job): the launcher is the oracle's third path beside the thin binary and the full router.
- **Sandbox measurement** (runbook, once): disk written and seconds from an empty target to the first `wf offload ask --help`, against the full build; logged in `docs/audits/2026-09-02-token-diet-decision-log.md`.

## Not decided

- Whether the launcher replaces `/tmp/wf-new-bin/wf` and the `mise` shim in the sandbox only, or the VM runner too (the CI jobs run the one wf build of the `clis` job, so they stay on the full binary).
- The staleness test: cargo's own freshness (always call `cargo build`, about 0.3 s when current) is simpler than step 2's file comparison and is the first version to build.

No implementation in this change: the table derivation and the lock handling are over 150 lines.
