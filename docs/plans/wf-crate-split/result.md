# wf crate split: validation result and recomputed estimate

Status: validated locally, 24 Oct 2026, on the working tree after every unit of the split (base commit `cde8a6d6e`, nothing committed or
staged by this unit). Test type for every pass below: **flow** (each crate's `tests/flow.rs`, a black box through the crate's thin
binary), then the same suites against the full `wf` (the parity oracle). There are no unit tests to report: every crate is
`[lib] test = false`, `doctest = false` (decision D3), so the 48 dormant unit tests stay dormant. Nothing here is an integration, service
e2e, browser e2e, Android or runbook run; no request went to the VM, GitHub or any cloud.

## Verdict

- Build: `cargo build --locked --offline --workspace` finished clean, no warning; `cargo test --workspace --no-run` prints none either.
- Flow: 162 of 162 tests pass, and the set of test names per crate equals `census.tsv` (162 rows: the plan counted 159 at its snapshot,
  and three cases were added since, the three-deep `nx_cache` case of W0, the per-crate plan case of W2 and the publisher test of W10).
  None failed, none ignored, none returned early.
- Parity oracle: all 142 tests of the 11 wf-* crates pass against the full debug `wf` (the 20 tests of the `wf` crate already drive it).
- Gates of the pre-commit and post-land hooks pass, with the one caveat on `--dead-refs` below.
- Build cost against the stop rule: cold deploy build 191.6 s against 205.2 s, leaf edit 8.7 s against 47.0 s. Passes
  (`baseline.md`, last section).
- Rerun estimate recomputed from the real graph: a commit that touches `clis/wf` reruns on average 88.7 of 162 flow tests (54.7%),
  median 73, and 22.2% of such commits still rerun all of them (section 1 of the plan estimated 51%, 70 and 22%).

## Flow suites

Run one after another with the PATH of `.agents/tmp/flow-path.txt` (no `wf` on it), `CARGO_INCREMENTAL=0`,
`cargo test --locked --offline --manifest-path clis/Cargo.toml -p <crate> --test flow -- --show-output`. Wall time includes the compile of
that crate's test.

| Crate | Census | Passed | Own binary | Parity (`WF_BIN` = debug `wf`) | Wall (own, s) |
| --- | --- | --- | --- | --- | --- |
| wf-secrets | 7 | 7 | ok | ok | 7 |
| wf-bucket | 3 | 3 | ok | ok | 6 |
| the ci crate | 6 | 6 | ok | ok | 3 |
| wf-vmops | 2 | 2 | ok | ok | 4 |
| wf-plan | 23 | 23 | ok | ok | 354 (the `nx_cache` tests copy the repo and run Nx) |
| deploy | 14 | 14 | ok | ok | 12 |
| offload | 10 | 10 | ok | ok | 10 |
| deploy | 18 | 18 | ok | ok | 51 |
| wf-bench | 5 | 5 | ok | ok | 38 |
| wf-login | 6 | 6 | ok | ok | 18 |
| wf-harness | 48 | 48 | ok | ok | 43 |
| wf | 20 | 20 | ok | not run (it is the full binary) | 44 |
| **Total** | **162** | **162** | | 142 | |

The five existing crates with a flow suite also pass: `agent-config-sync` 4, `blob-cache` 8, `cdp` 9, `cli-http` 3, `rca-core` 4 (not in
the census). `wf-kit` and `wf-testkit` have no `tests/flow.rs` (kit is proved by its dependants).

- Census check: for each crate the passed names equal the census names (no missing, no extra); the totals are equal (162 and 162).
- Skipped (returned early, counted as passed by libtest): **none**. `/opt/pw-browsers/chromium-1194` and
  `tooling/browser-harness/dist/index.html` exist here, so the 16 census rows marked `can_skip` (12 `browser::*` of `wf-harness`, the
  visual-tier test of `wf`, `harness::a_flow_file_with_update_goldens...`, the `wf-bench` real-browser test, the `wf-login` desktop test)
  ran for real. Evidence: `--show-output` printed no skip notice for any test of any run, and a second run of four of them with
  `--nocapture` took 13 to 15 s each with no notice. On a machine without Chromium or the built harness these 16 would pass without
  checking anything; the split does not change that (known gap, plan section 7).
- The first attempt at `http` failed with "cannot be tested because it requires dev-dependencies and is not a member of the workspace":
  `-p http` names the crates.io package, the workspace crate is `cli-http`. Re-run under that name, pass. It is a command typo, not a defect.

## Other gates

| Gate | Result |
| --- | --- |
| `wf check --budget` | pass (pre-commit jobs 3 of 5, root CLAUDE.md 74 of 150 lines, known-gaps 63 of 200, `wf-kit` 1,044 of 1,200 lines) |
| `wf check --dead-refs` | **0 findings on the tree as a commit would record it; 314 on the bare working tree.** The check reads `git ls-files`, and the 13 new crates are untracked until the orchestrator's commit, so every path into them looks dead. Run in a scratch repo that holds a copy of the working tree with the new files added (the real index was not touched): `ok: true`, 0 findings. Re-run it once after the commit |
| `pnpm run sync:check` | pass |
| `pnpm run arch:services` (backend files changed) | pass (ls-lint and ast-grep) |
| `actionlint .github/workflows/deploy.yml` (the only workflow changed) | pass |
| lefthook secrets filter (`WF_BIN` = debug `wf`, `cargo test -p wf-secrets --test flow secrets::the_value_revealing`) | 1 passed |

`wf check --staged` was not run (it needs staged files, and this unit stages nothing), nor lefthook as a whole.

## Recomputed estimate of section 1

Method. The graph is `cargo metadata` of `clis/Cargo.toml` (20 members); it equals the graph that `tooling/nx/cargo-crates.js` reads from
the manifests, dev-dependencies included, so `wf-testkit` (a dev-dependency of every crate with tests) makes a change in it global.
A crate's flow task reruns when a file of the crate changes (its directory, minus `target` and `*.md`), when a non-test file of a
transitive path dependency changes (`^rustProduction`: no `tests/`, no `benches/`), or when a file matching a fileset of its own
`project.json` changes (that input is the crate's own, not its dependants'). Tests per crate and seconds per test are the census
(162 tests, 1,037.7 thread-seconds measured at W0; 2 tests without a measured time count 0 s).

Replay. The 1,295 non-merge commits of `git log --since='30 days ago'` (read only). All predate the split, so a path under `clis/wf/`
is mapped to the crate that holds the file of the same name now (294 of the 304 files of that directory at HEAD; `tests/flow/support/*`
to `wf-testkit`, `fake_vm.rs` to `wf-vmops`, `browser_session.rs` and `mock_daemon.rs` to `wf-harness`). The same conventions as the plan:
`help_table.rs`, `main.rs` and `routes.rs` count as `wf` only; a module split over crates (`usage`, `e2e_tests`, `test_publish`,
`verify_tier3`) reruns every crate that holds a piece; every edit of the old `clis/wf/Cargo.toml`, `tests/flow.rs` or the old support is
global (pessimistic, as there). 200 files the window touched no longer exist at HEAD (deleted or renamed modules, 754 touches), so
no file maps them; the figures below use the crate of their siblings (same help noun, directory or name prefix, else `wf`), and the
brackets give the bounds: all of them in `wf` (lowest) or all global (highest). The script is a scratch file, not committed.

| Population | Commits | Tests rerun, mean | Median | Rerun all 162 | 30 tests or fewer |
| --- | --- | --- | --- | --- | --- |
| Touch `clis/wf` (the plan's population: 436 then) | 455 | **88.7** (54.7%) [88.2 to 108.6] | 73 [73 to 96] | 101 (22.2%) [22.2% to 42.0%] | 61 (13.4%) [64 to 32] |
| Rerun `crate-wf` today (its real inputs) | 593 | 81.9 (50.6%) [81.6 to 97.2] | 73 [73 to 79] | 121 (20.4%) [20.4% to 35.6%] | 146 (24.6%) |
| All commits | 1,295 | 39.2 (24.2%) [39.0 to 46.2] | 20 | 125 (9.7%) | 844 (65.2%) |

- Today every one of the 593 commits reruns all 162 (mean over all 1,295 commits: 74.2). After the split the mean over all commits is
  39.2, a 47% cut, and 79.6% of the commits that rerun the flow tests today rerun fewer. Weighted by W0's thread-seconds the mean rerun
  of the `clis/wf` population is 580 of 1,038 s (55.9%).
- The plan's figures were 81 of 159 (51%), median 70, 22% rerunning everything, 14% at 30 or fewer. The real graph is slightly worse
  on the mean (54.7%) and the same on the share that reruns everything, 22.2%. The prediction held.
- What reruns everything (101 commits of the `clis/wf` population, counted by trigger; a commit can have several): `wf-secrets` sources
  36, `wf-kit` 35 (+4 with `usage`), the lock 35, old support or `tests/flow.rs` 20, a `Cargo.toml` of the old `wf` 34, `registry.json`
  17 (a fileset of every crate), the workspace `Cargo.toml` 9. The planner (`crate_flows` in `test_plan.rs`) plans no crate for a change
  that touches only `clis/Cargo.lock` (a path under no crate directory), while Nx would invalidate every hash; the lock is counted as
  global here, as in the plan.
- The crate that holds `nx_cache` (`wf-plan`) reruns for 49.5% of the `clis/wf` commits (59.2% of the 593), against the plan's 38%: it
  also reruns on the `.github/**` and `mise.toml` filesets it gained, and it sits above kit, bucket and vmops.
- 79 of the 593 commits touch no file under `clis/` (a `package.json`, workflow, lua script, `mise.toml` and the like): they rerun
  `wf-plan` alone (70 commits, 23 tests) or `wf-plan` and `wf-harness` (9, for the `e2e/*.lua` files `wf-harness` lists), mean 28.5 tests
  against 162. The plan said the 121 contract-only commits would rerun 21 tests.
- 84 commits rerun a flow test now that reran none before, because the new `project.json` filesets list files the old `crate-wf` did not
  hash: 54 touch `.github/**` or `mise.toml` (the static scans of `wf-plan`), 10 `clis/app-bench/*.json`, 7 the secrets guard files, 4
  the EAS config or publish script, 2 `vnc-auth-capture`, 2 `registry.json`, 5 other files under `clis/` or the `wf` docs. Those are holes
  the split closes, and not a regression.

What a change in each crate reruns, from the real graph (plan section 5 in brackets, then 159 tests):

| Change in | Reruns the flow tests of | Tests |
| --- | --- | --- |
| `wf-kit`, `wf-secrets`, `wf-testkit` | every crate with tests | 162 [159] |
| `wf-vmops` | plan, vm, ask, harness, bench, login, ship, vmops, wf | 146 [142] |
| `wf-bucket` | bucket, ci, plan, ship, wf | 70 [66] |
| `wf-harness` | harness, bench, wf | 73 [70] |
| `wf-plan` | plan, ship, wf | 61 [56] |
| `deploy` | ship, wf | 38 [35] |
| `deploy` | vm, wf | 34 [34] |
| `offload` | ask, wf | 30 [27] |
| `the ci crate`, `wf-login` | ci or login, wf | 26, 26 [24, 23] |
| `wf-bench` | bench, wf | 25 [23] |
| `wf` | wf | 20 [17] |
| `rca-core` / `cli-http` | 10 or 11 wf crates | 152 / 155 |
| `cdp` / `telemetry` / `agent-config-sync` | harness, bench, login, wf / login, wf / wf | 79 / 26 / 20 |

The differences from the plan are the three tests added since and the real assignment of tests (`wf-plan` 23 against the plan's 21).
`wf-vmops` has a dev-dependency on itself (its `testing` feature), and so has `wf-harness`; the `nx_cache` suite, which runs Nx on a copy
of the repo, passes with both.

## What was not run

- The musl deploy binary `install-wf` builds, the persistent-incremental deploy build of CI, and the loaded VM (no musl target here).
- CI, the plan job's real `wf test plan` for a push, the `flow-tests` stage wall time of a docs-only, `ci_comment`-only and `wf-plan`-only
  commit (plan W6 and W13 asked for it; it needs the VM runner), and the Docker image of the listener.
- Any integration, deployment verification, browser e2e, Android or runbook test; `terraform plan`; the flow suites of the backend crates that
  depend on `rca-core` and `telemetry` (the path-dependency edge is in the table above).
- `wf check --staged`, lefthook as a whole, and `wf check --dead-refs` against a real index (see the gates table).
- The whole flow suite under the load of a 2-CPU CI job: these runs had four CPUs, `cargo` and libtest at their defaults.
