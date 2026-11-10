# Split wf into crates along its seams

Status: **done, 24 Oct 2026, landed as one commit** (owner rule of 23 Oct: no fallbacks, no re-exports at old paths, dead code
removed as it moved; the waves W0 to W13 of section 8 ran as units on one tree, not as commits). Validated locally at the flow tier
only: `result.md` and the last table of `baseline.md` (162 of 162 flow tests, the parity oracle for the 11 split crates, build cost
inside the stop rule). CI, the VM and the musl build have not run it yet. Plan written 23 Oct 2026 by the design agent. Owner
decision (23 Oct, binding): split `clis/wf` into crates so cargo's dependency graph, which Nx already follows, is exact; a change
reruns only the crates that depend on it.

Decisions taken (owner rule: act, do not ask): **D1** thin per-crate test binaries over one shared dispatcher in `wf-kit`;
**D2** (superseded 2026-10-26: the harness is split into `harness-core`, `harness-browser`, `harness-android` and `harness-flow`, `wf-harness` is only their facade; the crate layout after the CLI restructure is in `docs/audits/2026-09-02-token-diet-decision-log.md`) the harness stays one crate (`wf-harness`); **D3** the 48 dormant unit tests stay dormant (`[lib] test = false`,
`doctest = false`; they sit in 20 files and still do not run).

Deviations from this plan, each with its reason:

1. One commit instead of a commit per wave, so the section 8 rollback is a single revert and the stop rule (section 7) was judged
   once, on the final tree, not at W2. Why: the owner's one-commit rule.
2. 162 flow tests, not 159: three were added, the three-deep `nx_cache` case (W0), the planner case
   `a_change_in_a_wf_crate_plans_the_flow_tests_of_exactly_the_crates_that_build_it` (the rerun-table test of W6, built early) and
   `a_browser_run_told_to_publish_leaves_its_bundle_in_the_bucket_through_the_publisher_wf_sets` (W10). The per-crate counts come
   from `census.tsv`, not from the plan's regex guess: wf-plan 23 (plan 21), deploy 14 (17), wf-vmops 2 (none), wf-harness 48 (47),
   wf-bench 5 (6), the ci crate 6 (about 7), wf 20 (17). Why: section 4 point 3 assigns a test to the lowest crate whose thin binary serves
   every command it spawns; `vm exec` and `vm net resolve` are served only by `wf-vmops`, so its two cases live there.
3. The `vnc-auth-session` trigger paths are a fixed list, `DESKTOP_IMAGE_PATHS` in `clis/pipeline/src/legs.rs`, not derived from
   the listener's cargo closure at plan time (section 5, W12). A `deploy` flow test compares it with the Dockerfile's COPY lines
   and with the path closure of the listener's manifest. Why: the listener is its own workspace (own `Cargo.lock`), outside
   `clis/Cargo.toml`, so the workspace metadata the planner reads does not list it.
4. The 14 `registry.json` descriptions that named `clis/wf/...` paths were reworded to module names (the option section 5
   preferred), not pushed: the Bitwarden notes they mirror keep the old text until `wf secret push-notes` runs with the
   write-capable vault credential, which this work could not use.
5. `clis/Cargo.toml` sets `[profile.dev] debug = "line-tables-only"` (section 5 made it optional, "if W2 shows large test
   binaries"): every extra crate adds a flow and a thin binary to the debug dir. The debug rows of `baseline.md` (full debuginfo)
   and of the split tree are therefore not like for like.
6. Help is byte-identical except two lines: 382 of 384 captured `--help`, `-h`, `help <cmd>` and `--version` outputs match the
   pre-split binary (the 19 bare-noun captures were not re-run); `wf android list-devices --help` and `-h` print the catalog path
   `clis/wf-harness/src/android_features.json`, because the file moved with its crate.
7. Not run, although a done-when names them (nothing here may reach CI, the VM or a cloud): the `terraform plan` of W3, the
   `vnc-auth-session` and backend image builds, the musl `install-wf` build, `wf test plan --changed` on a real push, and the
   `flow-tests` stage time of a docs-only, a `ci_comment`-only and a `wf-plan`-only commit (W6, W13). The `deploy.yml` timing
   comment says the stage times of the split are not measured yet.

Result against the estimate of section 1 (`result.md`): a commit that touches `clis/wf` reruns 88.7 of 162 flow tests on average
(54.7%, median 73, 22.2% rerun all), against the 51%, 70 and 22% estimated; over all 1,295 commits of the window the mean is 39.2
against 74.2 before. The real graph is slightly worse on the mean and the same on the share that reruns everything.

Snapshot: branch `claude/device-lab`, HEAD `cde8a6d6e` (the edits to `deploy.rs`, `test_plan.rs`, `vm_ops.rs`, `verify_tier3.rs`
and the tests `check.rs`, `ship.rs`, `vm.rs`, `expected_revisions.rs` landed while this plan was researched). Line counts are
`wc -l` of that tree, blanks and comments included. 159 flow tests at HEAD (`^\s*#\[test\]` over `clis/wf/tests`); the CI log
quoted below is from the parent commit `ef90df980`, which ran 152. Nothing here was compiled (owner rule: no cargo build or
test); every claim marked "unverified" is listed in section 10.

## 1. Goal and evidence

Goal: a change to one area reruns that area's flow tests and its dependents' only, and no more than today anywhere.

Evidence for the problem (CI log `.agents/tmp/flow-now.txt`, 23 Oct, dev `ef90df980`):

| Fact | Source |
| --- | --- |
| `crate-wf` cargo-flow: 152 passed, finished in 614.06 s | flow-now.txt L2008 |
| Nx stage: run duration 10m 36s, cache 9/10 hit, critical path 10m 35s (1 task) | L2016-2018 |
| Six of the seven `nx_cache::*` tests ran over 60 s each: at least 360 of about 1,228 thread-seconds (29%) | L1917-1927 |
| One test binary (`tests/flow.rs`, 22 `#[path]` modules, 16,701 test lines) drives one bin through `CARGO_BIN_EXE_wf` | `support/run.rs:8-13` |
| Nx hashes `{projectRoot}/**` plus `^rustProduction` of dependencies, so any file in `clis/wf` or any of its inputs reruns every flow test (152 then, 159 now) | `nx.json` `rust`, `rustProduction`; `clis/wf/project.json` |

Who reruns it today (30 days, `git log --since='30 days ago' --no-merges`): 714 of 1,300 repo commits touch an input of
`crate-wf`: 455 touch `clis/wf`, 121 touch only a contract input (`package.json`, `project.json`, `Cargo.toml`, `e2e/*.lua`,
`nx.json`, `tooling/nx`) and 178 touch only another crate under `clis/`. Each reruns every flow test.

Churn that decides the order (436 commits touching `clis/wf`): help texts 127, `device` 63,
`browser_daemon` 50, `deploy` 48, `run` 37, `android` 36, `browser` 33, `secrets` 32, `android_input` 26, `verify` 25,
`build_image` 24, `app_bench` 24, `e2e_tests` 21, `verify_tier3` 21, `test_plan` 16, `vm_ops` 15, `test_metrics` 15, `vmops`
14, `test_publish` 13, `vm_exec` 13; `cache` and `ci_comment` are under 7 each. The owner's five areas (cache, ci comment,
test publish, vm ops, ship) are the right leaves, but the hottest code is the device harness and deploy/ship; the design
serves both.

Expected result (estimate, not a run): replaying the 436 commits against the crate graph below, with each test file assigned
to one crate, a commit reruns on average 81 of 159 tests (51%; 48% if each `nx_cache` test counts as 8 ordinary ones),
median 70; 63 commits (14%) rerun 30 tests or fewer; 97 (22%) still rerun all 159 (a change in kit, secrets, the test kit,
`Cargo.toml` or `Cargo.lock`; the model counts every `Cargo.toml` edit as global, which is pessimistic). The crate that holds
`nx_cache` reruns for 38% of commits instead of 100%. The 121 contract-input-only commits rerun the plan crate's own 21 tests
instead of 159. Script: scratchpad `design/est.py` (not committed); the same numbers must be recomputed at wave W13 from the
real graph.

## 2. Target crates

Directory rule: sibling crates `clis/wf-<name>`, same depth as `clis/wf`. Every relative `include_str!`/`include_bytes!`
path (`../../cdp/src/js/*.js`, `../../cm6/cm6.mjs`, `../../../secrets/src/data/registry.json`, `../help/...`,
`../assets/u2.jar`) and every test's `CARGO_MANIFEST_DIR/../..` stays valid if a module moves with its directory layout
intact (`git mv clis/wf/src/secrets clis/wf-secrets/src/secrets`). The package and binary named `wf` stay at `clis/wf`:
`-p wf` appears in `install-wf`, `lefthook.yml`, `architecture.yml`, `setup.sh`, `postinstall.mjs`, `secrets/mod.rs:364` and
its test.

Each crate is a lib (`[lib] test = false`, `doctest = false`: the 48 dormant `#[cfg(test)]` tests stay dormant, see D3) plus
a thin test binary `src/bin/wf-<name>.rs` (`test = false`) that serves only the commands the crate's tests spawn. Public
entry of a crate: its handler functions (`fn(&[String]) -> Option<i32>`, the type of `routes.rs:8`), `pub const HELP`, and a
`pub const COMMANDS` the thin binary mounts. `routes.rs` in `wf` stays the one real router.

| Crate | Receives (modules, lines) | Flow tests that move (HEAD estimate) | Direct path deps |
| --- | --- | --- | --- |
| `wf-kit` (about 1,000) | `args` 276, `json_mode` 103, `time` 31, `root` 14, `env_file` 150, `logfmt` 24 plus `slot::{logfmt,info}` (about 30), `js` 18 plus `dispatch_run::js_num`, `stop` 38; `usage` reduced to `wf_err!` and `note_error` (about 35 of 161); the help engine of `help.rs` (about 90 of 108: `find`, `serve`, `command_path`, `text`, `is_top_level`, over a table registered at start); `cli` (the dispatch of `main.rs`, about 90); new `proc` (about 80: `git_out` from `verify.rs:423`, `capture` from `verify_tier3.rs:70`, `capture_code` from `android_lab.rs:119`, `hostname`, `current_branch`) | none (proved by dependents) | none |
| `wf-testkit` (1,520 moved from `tests/flow/support`) | `run`, `stubs`, `mock_server`, `mock_tailnet`, `mock_https`, `pki`, `slot_sandbox`, node-script helper | none | none (dev-dependency of every crate with tests) |
| `wf-secrets` (2,157, 11 files) | `secrets/` incl. `groups.json`, `Backend` port | secrets 7 | kit |
| `wf-bucket` (about 700) | `cache.rs` 130 + `cache/` 261, `cache_runs` 76, `nx_drift::pins` (about 50, moved), the bucket helpers of `verify_tier3` (`OCI_BUCKET_BASE`, `put_par`, `read_public`, `content_type_for`) and `test_publish` (`existing_index`, `par_url`, `public_base`), about 180 | cache 3 | kit, secrets, blob-cache, cli-http |
| `the ci crate` (about 950) | `ci_comment` 349, `test_publish` (about 234 after the bucket helpers leave), `test_metrics` 305, `test_goldens` 67, `run_test` (the `publish` and `goldens` arms of the `wf test` router) | ci_comment 3, publish/results/goldens cases of `pipeline`, `report::goldens`: about 7 | kit, secrets, bucket, cli-http |
| `wf-vmops` (about 730) | `vmops` 200, `vm_exec` 308, `vm_sampler` 82, `tailnet_resolve` 97, plus `running_commit`, `RECOVER_APPS`, `ps` (from `deploy.rs`/`vm_ops.rs`), `android_lab::IMAGE`; feature `testing` = `FakeVm` (233) | none (FakeVm is used by 66 tests in 8 files) | kit, secrets |
| `wf-plan` (about 2,200) | `verify` 549 (pnpm graph, affected, gate, budget; `--staged`, `--budget`), `nx` 113, `nx_drift` 180, `test_plan` 407, the catalog half of `e2e_tests` (`E2eTest`, `parse_test`, `select`, `pick`, `plan`, `run_stage`, `repo`, `git`, `head_sha`, `unit_key`, `args_key`, about 300 of 477), `lock_closure` 264, the planning half of `deploy.rs` (legs, `affected_since`, `app_changed_since`, `expected_revisions`, about 300 of 375), the image table and `app_image` of `build_image` (about 60), `cm6` 48; `project.json` with today's contract inputs | nx_cache 7, pipeline plan/run-stage cases (about 6), check staged/budget (about 7), expected_revisions 1: about 21 | kit, secrets, bucket, vmops |
| `deploy` (about 3,100) | `vm_ops` 224, `vm_logs` 390, `vm_recover` 291, `vm_drill` 176, `agent_deps` 240, `infra` 490, `tailscale` 450, `tailscale_up` 152, `report_run` 613, `report_health` 59 | vm 7, vm_drill 3, tailnet 4, agents `vm deps` 2, report health 1: about 17 | kit, secrets, vmops |
| `offload` (3,311) | `dispatch` 243, `dispatch_batch` 188, `dispatch_run` 910, `cooldown_mirror` 65, `pricing_sync` 104, `runbook` 188, `worker_plan` 494, `worker_run` 1,119 | ask 8, worker 2 | kit, secrets, vmops, rca-core |
| `deploy` (about 1,800) | `ship` 77, `ship_eas` 275, `deploy` run/status/retire (about 75), `deploy_run` 351, `build_image` scripts (about 229), `verify_tier3` (about 776 after the bucket helpers leave: `check --deployed`, `wait_serving`) | ship 15, deploy_legs 1, check `--deployed` 2: about 18 | kit, secrets, vmops, bucket, plan |
| `wf-harness` (about 12,370, 25 modules, 33 files) | `device/` 4,455 (mlua only here), `android*` (6 modules), `browser*` (6 modules), `golden`, `pixelmatch`, `spans`, `run`, `run_report`, `run_stop`, `slot`, `staticd`, `stub_server`, `tailnet_forward`, `tailnet_url`, `trace`, `video_check`, `android_lab`, `android_lease`; `help/wf__run__api.txt`, `assets/u2.jar`; feature `testing` = `MockDaemon`, `BrowserSession` | browser 17, android 15, harness 11 (minus about 3 that drive `flow run`), maestro 7: about 47 | kit, vmops, cdp |
| `wf-bench` (2,016) | `app_bench/` 1,764, `bench` 252 | bench 6 | kit, harness |
| `wf-login` (1,688) | `vnc/` 1,385 (axum, tower-http, tokio here only), `auth` 303 | auth 6 | kit, secrets, vmops, cdp |
| `wf` (about 4,300) | `main`, `routes`, `usage` recorder (`record`, `send_if_asked`, about 125), `dead_refs` 413, `agents` 23, `knowledge` 445, `knowledge_gemini` 369, `log_emit` 23, `agent_proof` 216, and the e2e runner family: `e2e_run` 367, `export_site` 308, `flow_run` 115, `flow_goldens` 156, `visual` 916, `test_results` 331, the device-run half of `e2e_tests` (about 150) | surface 4, dead-refs 2, usage 1, mcp-guard 1, knowledge 2, log emit 1, agents 2, e2e family about 6, plus the parity run: about 17 | all of the above, agent-config-sync, telemetry |

Sum 36.3k lines (36.1k today plus about 200 of new glue). Existing crates are used where the code uses them: `blob-cache`
only in bucket, `cdp` in harness and login (and `visual` in wf), `rca-core` in ask, `telemetry` and `agent-config-sync` in
wf; the compiler names the rest in each wave (unverified: which third-party crate goes where beyond the heavy ones).

Layers (the DAG as text):

```
L0  wf-kit
L1  wf-secrets            (kit)
L2  wf-bucket             (kit, secrets)          wf-vmops   (kit, secrets)
L3  the ci crate                 (kit, secrets, bucket)
    wf-plan               (kit, secrets, bucket, vmops)
    deploy, offload, wf-login   (kit, secrets, vmops [, rca-core | cdp])
    wf-harness            (kit, vmops, cdp)
L4  deploy               (kit, secrets, vmops, bucket, plan)
    wf-bench              (kit, harness)
L5  wf                    (all)
dev: wf-testkit in every crate with tests; wf-vmops[testing] in the crates that use FakeVm; wf-harness[testing] likewise
```

Why this partition and not another (data from the static graph, all unverified by a build):

- Today 71 of 96 top-level modules (31,792 lines) are one strongly connected component. Seven import edges into the routers
  from below make it: `usage -> help::command_path, routes::is_command, secrets::load_values`; `args -> help::command_path`
  (`args.rs:120`); `help -> device::api::{noun_section,verb_help,by_verb}, device::cli::is_verb` (`help.rs:50-64`);
  `android`/`browser -> help::text`; `dead_refs -> routes::is_command`. With those cut, 36 item references over 26 module edges
  remain (section 3). Strictly nothing moves by file moves alone.
- Heavy dependencies already sit in one area each: `mlua` (vendored Lua 5.4) only in `device/lua.rs`; `axum`/`tower-http`
  only in `vnc`; `ed25519-dalek` only in `vmops`; `chacha20poly1305` only in `secrets`; `blob_cache` only in `cache`;
  `sourcemap` in `browser_debug` and `app_bench`. Splitting makes a crate that does not need them stop compiling and linking them.
- `help_table.rs` (113 help files, 106 KB) is not in the kernel: every crate owns the help of its commands and registers it
  at start, so a help edit (127 commits in 30 days, today a full rerun) reruns one crate and the thin tier.
- `verify` and `verify_tier3` are a 2-cycle. `verify` reaches `verify_tier3::{SPEC,run,validate}` only to pick its mode and
  dispatch (`verify.rs:468,528,537`); `verify_tier3` uses 10 items of `verify` (`APP_PATHS`, `FRONTEND_APP`, `GATE_STEPS`,
  `HTTP_HEALTH_APPS`, `affected`, `apps_for_paths`, `filters`, `git_out`, `is_code`, `workspace_packages`). The dispatch moves up
  into `routes.rs::check`, which already splits on `--deployed`, so `verify_tier3` (in ship) depends on `verify` (in plan) and
  not the reverse. It goes to ship because it needs `ship_eas` and ship needs its `wait_serving`.
- Harness is one crate, not three: splitting it into core, android and browser lowers the mean rerun from 80.8 to 75.9 tests
  per commit (estimate, same script) for 10 more seam items (`browser::{hostname,screen_size,sessions}`,
  `BrowserDevice`/`AndroidDevice` wiring in `run.rs` and `device/cli.rs`, `trace::decode_rgba`, `video_check::input_log_path_for`,
  `android::{serial_or_env,utc_z}`, `android_lease::renew_while_running`). See D2.

First pilot: `wf-kit` + `wf-testkit` + `the ci crate` holding `ci_comment` alone (wave W2). `ci_comment` is the most
self-contained area: 349 lines, imports only `args` (and `wf_err!`, `cli_http`), no dependants except `routes.rs`, no non-Rust
files, 3 tests (211 lines, one `MockServer` as the GitHub API). It still forces everything that can go wrong in the pattern:
the kit edges (help, usage, args), a thin binary and `CARGO_BIN_EXE_pipeline`, the shared test kit, the Nx task and the planner
picking it up, the `Cargo.lock` stanzas, the build of the musl deploy binary. Order after the pilot, by number of
dependencies, so each wave compiles on what is already a crate: secrets (kit) -> bucket and finish ci (kit, secrets) -> vmops
(kit, secrets) -> plan (kit, secrets, bucket, vmops) -> vm, ask (kit, secrets, vmops) -> ship (plan) -> harness (kit, vmops;
biggest, so after the pattern is proven) -> bench (harness) -> login (vmops; last because the listener image and the deploy
leg depend on it). Plan comes before harness because `nx_cache` (the slowest tests) leaves `wf` there.

## 3. Seams: what must change before a module can move

Each wave does its seam edits first, as a commit that still builds as one crate and passes the unchanged suite, then the move.
The 26 module edges of the minimum feedback set (about half are a command family calling its own sub-handler and vanish when
the family stays in one crate) and the kernel edges resolve as follows.

| Edge today | Resolution | Wave |
| --- | --- | --- |
| `args -> help::command_path` (`args.rs:120`); `usage -> help::command_path` | the help engine moves into kit and reads a table registered at start (`help::register(&[&HELP...])`, a `OnceLock`); the listener registers one row and drops its stub | W1 |
| `usage -> routes::is_command`, `secrets::load_values`, `env_file::load`, `crate::VERSION` (`usage.rs:55-86`) | `record` and `send_if_asked` (about 108 of 161 lines, called once each from `main.rs:103,124`) move to `wf`; kit keeps `wf_err!` and `note_error`; `VERSION` is passed into `cli::run` so `env!("CARGO_PKG_VERSION")` is read in `wf` | W1 |
| `help -> device::api::{noun_section,verb_help,by_verb}`, `device::cli::is_verb` | harness registers two hooks (`noun_section`, `verb_help`) with the kit help engine | W1, W10 |
| `android`/`browser -> help::text`; `dead_refs -> routes::is_command, android::VERBS, browser::VERBS, device::api` | `help::text` is kit; `dead_refs` stays in `wf`; `verify::run` receives the dead-refs scan as a function argument from `routes::check` (`verify.rs:372`) | W1, W6 |
| `browser::hostname`, `browser::current_branch`, `dispatch_run::js_num`, `slot::{logfmt,info}`, `android_lab::capture_code`, `verify::git_out`, `verify_tier3::capture`, `stop` | to kit (`proc`, `js`, `logfmt`, `stop`); the two logfmt writers keep their two outputs unchanged (no merge) | W1 |
| `run -> test_publish::publish` (`run.rs:107`) | harness exposes a publisher hook (`OnceLock<fn(&Path,&str,bool)->Result<..>>`); `wf` registers it; no harness test drives `--publish` (`harness.rs` has none; unverified for the others), one `wf`-tier test covers `browser test --publish` | W10 |
| `run`/`android`/`browser -> run::test_for`, `android_lease -> android::container`, `android_lab <-> vm_exec` | stay inside harness / `capture_code` in kit | W1, W10 |
| `ship_eas`, `ship`, `test_publish`, `test_goldens -> verify_tier3::{wait_serving,capture,put_par,read_public}`, `test_publish::{par_url,public_base}` | bucket helpers to `wf-bucket`; `wait_serving` stays with `verify_tier3` in ship; `capture` is kit | W4, W9 |
| `cache -> verify::git_out`, `cache -> nx_drift::pins` | `git_out` is kit; `pins`, `Pin`, `mise_pins`, `installed` move to bucket (`nx_drift` then uses bucket) | W4 |
| `test_plan`, `nx_drift`, `export_site`, `test_results -> e2e_tests::{E2eTest,repo,git,head_sha,unit_key,args_key}`; `e2e_tests <-> test_results, e2e_run` | `e2e_tests.rs` splits three ways: catalog and plan to `wf-plan`; the `wf test` router (`e2e_tests::run`, `e2e_tests.rs:440-477`) to `routes.rs`, calling `plan`, `ci::run_test`, and the e2e family; device-run helpers (`UpdateTarget`, `runner`, `stop_run`, `recordings_in`, `report_in`) stay in `wf` | W6 |
| `test_plan -> deploy::expected_revisions` (landed in `cde8a6d6e`, `test_plan.rs:287`); `deploy -> build_image::app_image`, `lock_closure` | the planning half of `deploy.rs` and the image table move to `wf-plan` so plan does not depend on ship; `expected_revisions` reads the running commit through `vmops::running_commit` | W5, W6 |
| `verify -> verify_tier3::{SPEC,run,validate}` (`verify.rs:468,528,537`) | mode probe and dispatch move to `routes::check` (about 15 lines); `verify_tier3` goes to ship | W6, W9 |
| `verify -> cm6` re-exec `repo bundle --check` (`verify.rs:379`) | `cm6` (48 lines) goes to plan; the plan thin binary serves `repo bundle` | W6 |
| `vm_exec -> android_lab::capture_code`; `vm_recover -> deploy::{RECOVER_APPS,ps}`; `deploy`/`verify_tier3 -> vm_ops::running_commit` | `capture_code` to kit; `RECOVER_APPS`, `ps`, `running_commit` (deploy has a private copy at `deploy.rs:131`, merge them) to `wf-vmops` | W5 |
| `build_image -> android_lab::IMAGE` | constant to `wf-vmops` | W5 |
| `report_run <-> report_health`, `worker_plan <-> worker_run`, `browser_daemon <-> browser_launch`, `secrets` mod `<->` writers, `vnc` engine `<->` relay, `android_input <-> android_u2`, `app_bench` mod `<->` suites | stay inside one crate | none |
| `worker_plan -> visual::run` (`worker_plan.rs:474`), `visual -> worker_plan::repo_root` (`visual.rs:871`) | the `wf offload worker visual` forward moves to `routes.rs`; `visual` uses `root::repo_root` (7 repo-root helpers differ: `worker_plan`'s returns `Err` for a non-git cwd, check before swapping) | W8 |
| `auth -> vmops::tailnet_via` | login depends on `wf-vmops` | W12 |
| `slot -> browser::{read_registry,registry_dir,registry_path}` | both in harness | none |
| `app_bench -> dispatch_run` (`js_num` only) | kit | W1 |
| `agent_proof -> dispatch_run::dispatch_read_only` | `agent_proof` stays in `wf` | none |

Mounting rule for noun routers: `routes.rs` keeps `check`, `test`, `vm`, `net`, `secret`, `repo`, `ask`, `plan` and calls crate
handlers; `secret login` goes to `wf-login`, the rest to `wf-secrets`; `repo bench` to `wf-bench`, `repo bundle` to `wf-plan`,
`repo knowledge` and `repo sync` stay in `wf`.

## 4. How tests move

Decision basis: D1. Every test is a black box through argv. After the split each crate owns the tests of its area and
drives its own binary.

1. Thin binary per crate: `clis/wf-<name>/src/bin/wf-<name>.rs`, about 6 lines: `wf_kit::cli::run(&Surface { name,
   version, commands: wf_<name>::COMMANDS, help: &[wf_<name>::HELP], hooks: None })`. Cargo defines `CARGO_BIN_EXE_wf-<name>` for
   that crate's integration tests only, which is why each crate needs its own binary. The full `wf` calls the same `cli::run`
   with the full tables and the usage hooks, so there is one dispatcher. Thin binaries print no usage rows.
2. A thin binary mounts the commands its tests spawn, and the commands its code re-executes through `current_exe()` (13 sites
   today): `browser-daemon` in harness and bench (`browser.rs:195`, `app_bench/lab.rs:192`); `repo bundle` in plan
   (`verify.rs:379`); `ask` in ask (`worker_run.rs:532`); `secret` in vm (`report_run.rs:164`, `secret doctor`, ignored on
   failure); `wf test run` doubles through `WF_E2E_BIN` in plan (`test_plan.rs:365`); `visual.rs:405` and `usage.rs:99` are in
   `wf`; `device/lua.rs:229` and `device/mod.rs:271` only put the executable's directory on PATH.
3. Assignment rule: a test goes to the lowest crate whose thin binary serves every command it spawns. A test needing several
   crates goes to the lowest crate above all of them, else `wf`. W0 produces the exact table; the per-file guesses above come
   from a regex over first argv words and are not authoritative.
4. Split files by test, not by file: `pipeline.rs` (12) splits across plan, ci and `wf`; `check.rs` (10) across plan, ship and
   `wf`; `report.rs` (6) across vm, ci and `wf`; `agents.rs` (4) across vm and `wf`; `harness.rs` (11) across harness and `wf`.
   Test bodies and names stay unchanged; only `use` paths and the `support` module differ.
5. Shared support is `clis/wf-testkit`, a lib crate with its code in `src/` (not `tests/`): `^rustProduction` of a dependency
   excludes `tests/**` (`nx.json:62-66`), so a double kept under `tests/` would never rerun its dependants. Generic parts
   (`run_wf`, `spawn_wf`, `wait_until`, stubs, `MockServer`, `MockTailnet`, `MockHttps`, `pki`, `SlotSandbox`) live there;
   `rcgen`, `ureq` and `tiny_http` with `ssl-rustls` sit behind a `tls` feature so crates that need no TLS double skip them.
   Doubles follow their port (arch doc rule "the package that owns a port ships its double"): `FakeVm` in `wf-vmops`
   (feature `testing`), `MockDaemon`, `BrowserSession`, `have_browser` in `wf-harness` (feature `testing`).
6. The binary under test is a parameter, not `env!`: each crate's `tests/flow/support/mod.rs` (about 30 lines) re-exports
   `wf_testkit::*` and defines `wf_binary()` as `WF_BIN` if set, else `env!("CARGO_BIN_EXE_wf-<name>")`, plus
   `fixtures_dir()` and `repo_root()` from its own `CARGO_MANIFEST_DIR` (`clis/<crate>/../..` is the repo root for every
   sibling crate). The existing test files compile unchanged against that module.
7. Parity oracle: `WF_BIN="$(command -v wf)" cargo test -p wf-<name> --test flow` runs a crate's suite against the full binary.
   The full `wf` serves a superset of every thin binary's argv, so the override stays valid (`lefthook.yml:39` and
   `architecture.yml:78` already use it). Run for all crates in the post-land `rust-clis` job; it catches drift between a thin
   mounting and the real router.
8. What stays in `wf` as the thin integration tier (about 17 tests, all cheap): `surface` (4: help of every command, command
   table parity, refusals, version), `check --dead-refs` (2), the usage-row test, `mcp-guard`, `repo knowledge` (2),
   `log emit`, the `agents` tests that need the whole router, the e2e family tests (`flow run`, `test run`, `test results`),
   and one test that `wf browser test --publish` publishes (the hook). `surface` stops regex-parsing one `help_table.rs`
   and scans `clis/wf*/src/help_table.rs`.
9. Whole-repo contract tests: `nx_cache` (7) and `deploy_legs` (1) copy the repo. `nx_cache` goes to `wf-plan` (it exercises
   `test_plan`, `nx`, `nx_drift`, `cache::Shim` and the e2e probe, which are all there after W6) with the contract inputs of
   today's `clis/wf/project.json`. `deploy_legs` goes to ship with `ship`'s own inputs. Their `CRATES` list and expected
   sets (`nx_cache.rs:23,208,399,416-421`, the file-to-e2e table `:249-264`) are rewritten for the new crate list.
10. Per-crate `project.json` lists the non-Rust files that crate's tests read, as the repo files they open
    (`repo_root().join(..)`, `CARGO_MANIFEST_DIR` joins): wf-secrets `clis/secrets/src/data/registry.json`, `deny.json`,
    `scripts/guards/block-secret-exposure.mjs`, `**/.env.defaults`; deploy `clis/vnc-auth-capture/Dockerfile`,
    `frontend/mla-tamagui/app.config.js`, `scripts/publish-eas-update.js`, `tests/flow/fixtures/deploy-scripts/*.sh`;
    wf-plan today's contract set plus `.github/**` and `mise.toml` (the static scans in `pipeline.rs`); wf-bench
    `clis/app-bench/*.json`; wf `docs/reference/wf-cli.md` and `clis/wf*/src/help/**`. Today these are holes in `crate-wf`
    (section 7); a crate listing them is no worse and usually better. Derive each list by grepping the moved test files in
    the wave (unverified list).

## 5. Nx and CI

No change to the mechanism: `tooling/nx/cargo-crates.js` already makes one `crate-<name>` project per workspace member with
`implicitDependencies` from `[dependencies]`, `[dev-dependencies]` and `[build-dependencies]`, and a `cargo-flow` target
(`cargo test --locked -p <name> --test flow --manifest-path clis/Cargo.toml`, cached, inputs `rust`, `^rustProduction`, the
workspace `Cargo.toml` and `Cargo.lock`) for each crate that has `tests/flow.rs`. So each new crate is its own Nx task with
its own cache entry the moment it has `tests/flow.rs` and is listed in `clis/Cargo.toml` `members` (read literally by the
plugin and by `test_plan.rs:120`: no globs). `wf test run --stage flow-tests` already runs one `nx run-many -t
test:flow,cargo-flow` over every planned crate (`test_plan.rs:295-307`); `crate_flows` (`test_plan.rs:166-204`) plans a crate
when a file under its dir changed or a path dependency's did, so a change in `wf-kit` plans every crate with tests.

What a change reruns (flow tests that rerun, HEAD estimate; today every row is 159):

| Change in | Reruns the flow tests of | Tests |
| --- | --- | --- |
| `wf-kit`, `wf-secrets`, `wf-testkit` | every crate with tests | 159 |
| `wf-vmops` (client, exec, sampler, FakeVm) | vmops dependants: plan, vm, login, ask, harness, bench, ship, wf | 142 |
| `wf-bucket` (cache, bucket helpers) | bucket, ci, plan, ship, wf | 66 |
| `wf-plan` (verify, test_plan, nx, deploy legs) | plan, ship, wf | 56 |
| `wf-harness` (device, android, browser, run) | harness, bench, wf | 70 |
| `deploy` (deploy, ship_eas, verify_tier3) | ship, wf | 35 |
| `offload` | ask, wf | 27 |
| `deploy` (vm_ops, vm_logs, vm_recover, infra, tailscale, report) | vm, wf | 34 |
| `the ci crate` (ci_comment, test_publish, test_metrics) | ci, wf | 24 |
| `wf-login` (vnc, auth) | login, wf | 23 |
| `wf-bench` | bench, wf | 23 |
| `wf` (routes, help root, usage, e2e family, dead_refs) | wf | 17 |
| a help text of one command | its crate and `wf` | 24-70 |
| `package.json`, `project.json`, `Cargo.toml`, `e2e/*.lua`, `nx.json`, `.nxignore`, `.gitignore`, `tooling/nx/**` | `wf-plan`'s own task only (the contract inputs are `wf-plan`'s `project.json` inputs, not part of a dependant's hash) | 21 |
| `clis/http`, `cdp`, `rca-core`, `blob-cache`, `telemetry`, `agent-config-sync` | their dependants only (for example `cdp`: harness, login, bench, wf) | varies |
| `clis/Cargo.lock`, `clis/Cargo.toml` | every cargo-flow (unchanged; 45 commits in 30 days touched the lock) | 159 |

The plan job cannot run Nx (no `node_modules`, `deploy.yml:56-78`), which is why `test_plan.rs` reads manifests itself; the
rerun table above becomes a flow test in `wf-plan` (W6): for every crate it touches one source file in a fixture copy and
asserts the planned set equals the table's row.

Edits, by file:

| File | Edit | Wave |
| --- | --- | --- |
| `clis/Cargo.toml` | `members` gains each crate; optional `[profile.dev] debug = "line-tables-only"` if W2 shows large test binaries | each |
| `clis/Cargo.lock` | one `[[package]]` stanza per crate, `wf`'s dependency list shrinks; third-party versions unchanged; lands with each wave (`--locked`) | each |
| `clis/wf/src/test_plan.rs:182-183` | `c.name == "wf"` becomes `"wf-plan"` (the contract suite lives there); `shapes_the_nx_graph` unchanged | W6 |
| `clis/wf/project.json` | the contract inputs (`nx.json`, `.nxignore`, `.gitignore`, `tooling/nx/**/*`, `e2e/*.lua`, `**/package.json`, `**/project.json`, `**/Cargo.toml`) move to `clis/pipeline/project.json`; the comment "keep in step with `shapes_the_nx_graph`" moves with them | W6 |
| `nx.json` | none (named inputs are per project root); `.nxignore` none | none |
| `tooling/nx/cargo-crates.js` | none; a doc comment that `members` must list new crates | none |
| `tooling/nx/e2e-tests.js:12-30` | same files, new prefix: `clis/wf/src/{run,device/*,golden,pixelmatch,android*,browser*,staticd}` become `clis/wf-harness/src/...`; `export_site.rs` stays `clis/wf/src/export_site.rs`. No widening (cargo-exact inputs would add `trace.rs`, `run_report.rs`, `video_check.rs`, `stub_server.rs`, `spans.rs`, which today's list omits; a follow-up) | W10 |
| `tests/flow/nx_cache.rs` | `CRATES`, expected sets per edit, file-to-e2e table; a three-deep case | W0, W6, W10 |
| `clis/wf/src/verify.rs:394-406` (`APP_PATHS`) and `build_image.rs:63` | `vnc-auth-session` paths come from the listener's cargo closure (kit, secrets, vmops, login), as `lock_closure.rs` does for backend; today `APP_PATHS` omits the wf sources the listener compiles | W12 |
| `clis/wf/src/deploy.rs:72-86`, `clis/vnc-auth-capture/Dockerfile:22-35`, `tests/flow/ship.rs:211,221`, `deploy_legs.rs:118-119` | the vnc leg's `extra` list and the Dockerfile COPY lines name crate directories instead of `clis/wf/src/{vnc,secrets,args.rs,env_file.rs,root.rs,time.rs,browser_device_specs.json}` | W3, W12 |
| `clis/vnc-auth-capture/listener` | `Cargo.toml` takes path deps; `main.rs:5-36` loses its `#[path]` includes and its three stubs (`wf_err!`, `help`, `json_mode`); own `Cargo.lock` regenerated; states dependencies explicitly (telemetry precedent) | W3, W12 |
| `infra/oracle-cloud/main.tf:108`, `cloud-init.sh.tftpl:307-312`, `secrets/groups.rs:16` | `clis/wf/src/secrets/groups.json` becomes `clis/wf-secrets/src/secrets/groups.json` (rendered output unchanged) | W3 |
| `lefthook.yml:39` | `-p wf` becomes `-p wf-secrets` (filter `secrets::the_value_revealing` unchanged) | W3 |
| `lefthook.yml:55-58` | post-land `rust-clis` glob widens to `clis/**/*.rs`, `clis/**/Cargo.toml`, `clis/Cargo.lock` (today it misses `cdp`, `http`, `rca-core`, `blob-cache`) and runs the parity oracle | W13 |
| `.github/workflows/architecture.yml:78` | none (`surface::` stays in `wf`) | none |
| `.github/actions/install-wf/action.yml:51` | none: `cargo build --profile deploy -p wf` builds the whole path-dependency closure; the key `git rev-parse HEAD:clis` still covers every crate; the persistent incremental target dir recompiles only changed crates | none |
| `.github/workflows/clis.yml`, `deploy.yml` | none; update the timing comment at `deploy.yml:166-173` with the measured numbers | W13 |
| `.agents/setup.sh:70`, `scripts/postinstall.mjs:61`, `secrets/mod.rs:364` and `tests/flow/secrets.rs:390,406` | none (`cargo build --release -p wf`) | none |
| Docs | `docs/solutions/architecture/hexagonal-architecture-by-package-type.md` type 3 and its table row (wf becomes crates along seams; the `wf-core`/adapters target of `docs/plans/hexagonal-architecture/spec.md` W1-W9 and open question 12 is superseded by this decision), `.agents/instructions/root.md` CI paragraph and testing paragraph (then `pnpm run sync:agent-config`), `docs/solutions/testing/verification-ladder.md` (flow tests enter through the crate's thin binary), 77 live files that name `clis/wf/(src|assets|Cargo|tests)` (skills' `paths:` globs `clis/wf/src/browser*.rs` and `knowledge*.rs`, gap records' `paths:`, `docs/solutions/security/*`, infra READMEs, `registry.json` descriptions); `wf check --dead-refs` is the gate | each, W13 |
| `registry.json` | 15 path mentions (`vm_logs.rs` 8, `vnc/` 2, `vmops.rs` 2, `usage.rs`, `test_metrics.rs`, `secrets/`); the descriptions are mirrored as Bitwarden notes (`secrets/writers.rs:106`), so repointing them makes `wf secret push-notes` report updates that need the write token. Prefer rewording to module names with no path; otherwise run `push-notes`, which needs the write-capable vault credential (W3, W4, W7) | W3-W7 |

CI order is unchanged: `plan` (the previous push's wf) -> `flow-tests` -> `typecheck-lint-unit-tests` -> `clis` -> `deploy`.
Bootstrap: a wave's own commit plans its new crates because `crate_flows` plans a crate when a file under its dir changed, so
the old planner needs no change; only the push after W6 that touches nothing but Nx-graph files may miss `wf-plan` once (old
planner special-cases `wf`), so W6 lands with a no-op follow-up push to publish the new wf before the next such change.

## 6. What stays in wf, and what cannot be split

- Kernel (`wf-kit`): `args` is used by 48 modules, `wf_err!` by 44, `time` by 33, `root` by 18. Any kit change reruns
  everything, as today. Keep it about 1,000 lines, no help text, no HTTP, no vault, no process spawning beyond `proc`; add a
  `wf check --budget` entry capping it (proposal: 1,200 lines) and require three dependants for any addition. `wf-secrets`
  (17 users) and `wf-testkit` have the same property.
- `wf-vmops` is hot and low: 142 of 159 tests rerun on a change (27 commits in 30 days touched `vmops` or `vm_exec`). It is
  small (about 730 lines) and its callers are real: harness runs the Android lab on the VM through `vm_exec`, plan reads the
  running commit, ship builds on the VM. Splitting client from exec would spare plan, ask and login from `vm_exec` edits; not
  worth a crate now.
- `wf-bucket` is not a leaf: `cache::Shim` is used by plan, so a cache edit reruns 66 tests. Rare in practice (under 7
  commits in 30 days).
- Harness: one crate (cycle `device <-> android <-> browser <-> run` through driver wiring in `device/cli.rs`, `run.rs`,
  `android.rs`, `browser.rs`).
- Left in `wf` on purpose: `routes`, `main`, `usage` recorder, `dead_refs` (reads the whole command surface), `agents`,
  `knowledge`, `log_emit`, `agent_proof` (needs device and ask), and the e2e runner family (`e2e_run`, `export_site`,
  `flow_run`, `flow_goldens`, `visual`, `test_results`). The family sits above harness, plan, ship and ci, so a crate would rerun
  on 88% of commits instead of 100% for about 6 tests; extract it later as `wf-e2e` (kit, vmops, bucket, harness, plan, ship,
  ci) with no redesign. `auth` and `vnc` could have stayed too; they move because the listener image already compiles `vnc`
  from wf sources by `#[path]`, and a crate deletes that hack.
- Runtime coupling that cargo cannot see: 13 `current_exe()` re-exec sites (section 4, point 2), 167 `env::var` sites, files one
  command writes and another reads (`cache_runs`, the slot registry, `usage.error`). Parity relies on the unchanged suite
  and the `WF_BIN` oracle, not on the graph.

## 7. Costs and risks

| Item | Evidence and mitigation |
| --- | --- |
| Nx hash exactness | `^rustProduction` transitivity (A -> B -> C change reruns A) is inferred, not proven (the existing graph cannot distinguish it). W0 adds a three-deep fixture case to `nx_cache`; if it fails, each crate needs explicit `{projects}` inputs and the rerun table changes. |
| Compile and link on 2 OCPUs | Measured on one crate (`docs/audits/2026-09-02-token-diet-decision-log.md:2300,2360,2383`; `2026-10-17-cli-flow-tests-wf.md`): deploy-profile wf 181 s clean, 29 s incremental, 936 MB peak RSS on 2 pinned CPUs, 3.5 min incremental on the loaded VM; musl 2m08s cold, 17 s wf-only change; CI test build 20.63 s. N crates are unmeasured: expect overlap of crate frontends on a cold build, one link per changed binary, 12 more thin binaries and test binaries. Disk: debug `wf` 257.7 MB, flow test binary 113.7 MB, `clis/target` 3.9 GB, sandbox 5.6 GB free. W2 measures cold and incremental builds, thin and test binary sizes for the pilot; proposed stop rule: cold deploy-profile `cargo build -p wf` more than 25% above 181 s, or an incremental rebuild after touching `ci_comment.rs` above 29 s, halts the waves. Release builds (`setup.sh`, `postinstall.mjs`) use fat LTO and cost about the same whatever the crate count. |
| Full rerun is not faster | A kit, secrets or lock change reruns all 159 across 12 tasks at `--parallel 2`, plus extra links; wall time is about today's or slightly worse. The gain is on the other 78% of commits. Do not raise `--parallel`; a cold build of four crates already stalled the VM for 20 minutes (`deploy.yml:173`). |
| `Cargo.lock` churn | One stanza per new crate (about 12 lines each, estimate) and `wf`'s own entry (39 dependency lines today) is partitioned; each wave lands with its lock because cargo-flow uses `--locked`; the listener's own `Cargo.lock` (2,477 lines) is regenerated in W3 and W12. Shared third-party crates must keep today's versions and features (`serde_json` with `preserve_order` in kit, else it builds twice). No `[workspace.dependencies]`: crates the listener workspace builds cannot inherit it. |
| Help and surface tests | 113 help files move with their crates; `surface.rs` (reads `src/help`, regexes `help_table.rs`, reads `docs/reference/wf-cli.md`) must scan every crate's table; `wf <cmd> --help` output must stay byte-identical (the surface suite is the check). |
| Behaviour parity | The unchanged flow suite is the check. Per wave: test names after = test names at W0 (set equality), every crate green, `WF_BIN` parity green, `wf check --dead-refs` 0, a run with `wf` absent from the children's PATH (a test that shells out to `wf` by name would otherwise pass against the installed binary). |
| Visibility | 286 `pub(crate)` and about 586 `pub` items; cross-crate use needs `pub`. Re-exports at each crate root keep paths: `pub(crate) use wf_kit::{args, time, ...}` (the pattern of `main.rs:6`, `crate::http`). `crate::wf_err!` (99 lines in 45 files) via `pub(crate) use wf_kit::wf_err;`; if a path-based macro call does not resolve, rewrite the 99 sites to `wf_kit::wf_err!` (unverified). |
| Dormant unit tests | 48 `#[test]` in 20 files never run (`[[bin]] test = false`, cargo-flow runs `--test flow` only). `[lib] test = false` and `doctest = false` keep them dormant so the split changes no pass state; indented doc comments would otherwise be compiled as doctests. See D3. |
| Silent skips | About 12 of 17 browser tests and 1-2 auth tests return early (counted as passed) when Chromium or `tooling/browser-harness/dist` is missing; the flow job never builds `dist` (unverified). The split multiplies places a cached pass can hide one; not fixed here, W0's census records which tests skipped. |
| Planner and Nx drift | `test_plan.rs` and the Nx plugin must keep agreeing; the rerun-table test (W6) and the `nx_cache` rewrite pin it. |
| Churn while the waves run | 436 commits touched `clis/wf` in 30 days and the tree moved during this research (`cde8a6d6e` added the `test_plan -> deploy` edge). Start W0 from a fixed commit, keep each wave short, rebase the seam commit on `origin/dev` before the move commit. |
| Docker and CI builds that copy a partial `clis/` | `clis/vnc-auth-capture` is known (Dockerfile COPY and skeleton lines); which other Dockerfiles or scripts copy a subset of `clis/` was not checked (unverified). Backend services depend on `rca-core` and `telemetry` only. |
| Registry notes | See section 5, `registry.json`. |

## 8. Waves

Each wave is one or more commits on the `dev` trunk (rebase on `origin/dev`, lefthook suite, push; CI is the proof). Inside a
wave, the seam commit builds as one crate and passes the unchanged suite before the move commit. Every wave's done-when also
holds: `wf test plan --changed` for the push plans exactly the crates in its rerun-table row; the `flow-tests` stage is green;
`wf check --dead-refs` reports 0; the sum of tests across all crates equals the W0 census; `wf` builds as one musl deploy
binary (`install-wf`).

| Id | Wave | Files | Done-when |
| --- | --- | --- | --- |
| W0 | Census and proofs, no source change in `src` | `tests/flow/support/run.rs` (+about 15 lines: env `WF_TEST_ARGV_LOG` appends thread name, start and end ms, first three argv words per spawn); new `docs/plans/wf-crate-split/census.tsv`; `tests/flow/nx_cache.rs` (+1 case: a change two crates down reruns the dependant, three-deep fixture) | `census.tsv` has one row per test (count equals `cargo test -p wf --test flow -- --list`), each with crate (one of secrets, bucket, ci, vm, login, plan, ask, ship, harness, bench, wf), verbs, seconds, repo files read, skipped yes/no; all pass with `wf` absent from child PATH; the three-deep case passes (if it fails, stop and amend section 5) |
| W1 | Kernel seams in place (still one crate) | `usage.rs`, `help.rs`, `args.rs`, `main.rs`, `routes.rs`, `slot.rs`, `js.rs`, `logfmt.rs`, `browser.rs` (hostname, current_branch), `dispatch_run.rs` (`js_num`), `verify.rs`/`verify_tier3.rs`/`android_lab.rs` (`git_out`, `capture`, `capture_code` into new `proc.rs`), new `usage_row.rs` | `rg -n -e 'crate::help_table' -e 'crate::routes' -e 'crate::secrets' -e 'crate::device' -e 'crate::browser' -e 'crate::slot' -e 'crate::dispatch_run' -e 'crate::verify' -e 'crate::android_lab' clis/wf/src/{args,json_mode,time,root,env_file,logfmt,js,stop,proc,usage}.rs` prints nothing and `usage.rs` holds only `wf_err!` and `note_error`; `help.rs` reads a registered table; unchanged suite passes; `wf --help` and every `wf <cmd> --help` byte-identical (surface); usage-row test passes |
| W2 | Pilot: `wf-kit`, `wf-testkit`, `the ci crate` with `ci_comment` | new `clis/wf-kit/**`, `clis/wf-testkit/**`, `clis/pipeline/{Cargo.toml,src/{lib,ci_comment,help_table}.rs,src/help/wf__ci*.txt,src/bin/the ci crate.rs,tests/flow.rs,tests/flow/{ci_comment,support/mod}.rs}`; `clis/Cargo.toml`, `Cargo.lock`; `clis/wf/Cargo.toml`, `main.rs` (re-exports); `tests/flow/support/mod.rs` re-exports the test kit; `tests/flow/nx_cache.rs` sets | CI shows `crate-pipeline:cargo-flow` 3 passed and `crate-wf:cargo-flow` (census minus 3) passed; touching `clis/pipeline/src/ci_comment.rs` plans {the ci crate, wf} and touching `clis/wf-kit/src/args.rs` plans every crate with tests (new `nx_cache` cases); `WF_BIN=$(command -v wf) cargo test -p pipeline --test flow` passes; the measurements of section 7 are logged in `docs/audits/2026-09-02-token-diet-decision-log.md` and inside the stop rule |
| W3 | `wf-secrets`; listener on path deps for kit and secrets | `git mv clis/wf/src/secrets clis/wf-secrets/src/secrets`; `tests/flow/secrets.rs`; `lefthook.yml:39`; `infra/oracle-cloud/main.tf:108`, `cloud-init.sh.tftpl:307-312`; `secrets/groups.rs:16`; `deploy.rs:72-86`, `Dockerfile:22-35`, `ship.rs:211,221`, `deploy_legs.rs:118-119`; `listener/{Cargo.toml,Cargo.lock,src/main.rs}` | `crate-wf-secrets:cargo-flow` 7 passed; a `terraform plan` of `infra/oracle-cloud` shows no change to the rendered `cloud-init` (the moved file is byte-identical); the vnc-auth-session image builds (leg runs in the next deploy) and the listener has no `#[path]` to kit or secrets and no stubs for `wf_err!` and `json_mode`; `secrets::the_value_revealing` passes in pre-commit |
| W4 | `wf-bucket`; finish `the ci crate` | `cache.rs`, `cache/`, `cache_runs.rs`, `nx_drift::pins`, bucket helpers of `verify_tier3.rs` and `test_publish.rs` into `clis/wf-bucket`; `test_publish.rs`, `test_metrics.rs`, `test_goldens.rs`, `run_test` into `clis/the ci crate`; tests `cache.rs` and the publish/goldens cases | `crate-wf-bucket` 3 and `crate-pipeline` about 7 passed; `ship_eas`, `verify_tier3` import the bucket helpers from `wf-bucket`; touching `clis/wf-bucket/src/cache.rs` plans bucket, ci and wf (plan and ship join the set as they become crates) |
| W5 | `wf-vmops` with `FakeVm` | `vmops.rs`, `vm_exec.rs`, `vm_sampler.rs`, `tailnet_resolve.rs`, `running_commit`, `RECOVER_APPS`, `ps`, `IMAGE`; `tests/flow/support/fake_vm.rs` to `wf-vmops/src/testing.rs`; merge the two `running_commit` copies | no test of FakeVm moved yet; every test using `FakeVm` still passes; `rg -e 'vm_ops::running_commit' -e 'deploy::RECOVER_APPS' -e 'deploy::ps' -e 'android_lab::IMAGE' -e 'android_lab::capture_code' clis` prints nothing |
| W6 | `wf-plan`: `nx_cache` leaves `wf` | `verify.rs`, `nx.rs`, `nx_drift.rs`, `test_plan.rs`, catalog half of `e2e_tests.rs`, `lock_closure.rs`, planning half of `deploy.rs`, image table of `build_image.rs`, `cm6.rs`; router half of `e2e_tests.rs` into `routes.rs`; `test_plan.rs:182`; `clis/pipeline/project.json` (contract inputs); `clis/wf/project.json` shrinks; `nx_cache.rs`, `pipeline.rs`, `check.rs` cases move; rerun-table test | `crate-wf-plan:cargo-flow` about 21 passed, `crate-wf` no longer runs `nx_cache` (7); a commit touching only `package.json` plans `wf-plan` (and its dependants) and not harness or ask; the rerun-table test passes for every crate that exists; measured `flow-tests` stage time of a docs-only vs `ci_comment`-only vs `wf-plan`-only commit logged |
| W7 | `deploy` | `vm_ops`, `vm_logs`, `vm_recover`, `vm_drill`, `agent_deps`, `infra`, `tailscale`, `tailscale_up`, `report_run`, `report_health`; tests vm, vm_drill, tailnet, agents `vm deps`, report health; the help files of `vm`; `routes.rs` `vm`/`net` arms call the crate | `crate-deploy:cargo-flow` about 17 passed; `registry.json` notes for `vm_logs.rs` reworded or pushed; thin binary also mounts `secret` for `report_run`'s `secret doctor` |
| W8 | `offload` | `dispatch*`, `cooldown_mirror`, `pricing_sync`, `runbook`, `worker_plan`, `worker_run`; tests ask, worker; `worker visual` forward to `routes.rs`; `visual.rs:871` repo root | `crate-wf-ask` 10 passed; `wf offload worker visual` still runs through `wf` |
| W9 | `deploy` | `ship`, `ship_eas`, run/status/retire of `deploy`, `deploy_run`, scripts of `build_image`, `verify_tier3`; tests ship, deploy_legs, check `--deployed`; `routes::check` probe | `crate-deploy` about 18 passed; `check <sha>` and `--deployed` unchanged (check tests); deploy script fixtures byte-identical (`ship.rs`) |
| W10 | `wf-harness` with `testing` feature | `device/`, `android*`, `browser*`, `golden`, `pixelmatch`, `spans`, `run`, `run_report`, `run_stop`, `slot`, `staticd`, `stub_server`, `tailnet_forward`, `tailnet_url`, `trace`, `video_check`; `MockDaemon`, `BrowserSession`; publisher and help hooks; `tooling/nx/e2e-tests.js`; tests browser, android, harness, maestro | `crate-wf-harness` about 47 passed; the Nx e2e project hashes for `e2e-*` change only when a listed file did (same files as before, new prefix): `nx_cache` file-to-e2e table passes; a flow test in `wf` proves `browser test --publish` publishes |
| W11 | `wf-bench` | `app_bench/`, `bench.rs`, tests bench; thin binary mounts `browser-daemon` | `crate-wf-bench` 6 passed |
| W12 | `wf-login`; listener fully on path deps | `vnc/`, `auth.rs`; `listener` loses the last `#[path]`; `Dockerfile`, `deploy.rs` extras, `verify.rs` `APP_PATHS` from the cargo closure; tests auth | `crate-wf-login` 6 passed; the `vnc-auth-session` leg builds and serves its commit (CI `deploy` job); `ship.rs`/`deploy_legs.rs` leg assertions pass; `check --deployed` verifies the leg for a login-only change |
| W13 | Close | thin-tier tests in `wf`; `surface.rs` scanning all tables; `lefthook.yml` post-land glob and parity oracle; docs of section 5; `deploy.yml` timing comment; census equality; decision log | `cargo test --manifest-path clis/Cargo.toml --workspace -- --list` matches `census.tsv` names; parity oracle green for all crates; the section 1 estimate recomputed from the real graph and measured stage times of three commit types logged in `docs/audits/2026-09-02-token-diet-decision-log.md` with the decision; `wf` has no module the table assigns elsewhere (`ls clis/wf/src` matches the `wf` row) |

Rollback: each wave is revertable as a unit (seam commit and move commit); a failed stop rule at W2 abandons the plan with
only W0 and W1 landed, both of which are behaviour-neutral.

## 9. Open decisions for the owner

- D1. Test seam: thin per-crate test binaries (recommended) or an in-process `run(argv, world, io)`. The 17 Oct audit
  (`docs/audits/2026-10-17-cli-flow-tests-wf.md:26-35`) kept the black-box binary ("the one main") because handlers use
  process-global state (thread-local arg state in `args.rs:8`, `LAST_ERROR` in `usage.rs:14`, env, cwd, signals, exit code),
  which is unsafe in one libtest process. A thin binary per crate keeps the black box and the suite unchanged, at the price of
  12 small `main`s over one shared dispatcher; the full-`wf` parity run keeps "one main" as the oracle. In-process would
  rewrite every test.
- D2. Harness as one crate (recommended) or three (core, android, browser). Estimate: mean 80.8 versus 75.9 tests rerun per
  commit, a browser or android commit stops rerunning the other platform's tests (the android crate reruns for 51.8% of commits and the browser crate for 44.5%, versus 63.1% for one harness crate) for 10
  more seams and a driver registry. Revisit after W10 if browser-only and android-only commits dominate harness churn.
- D3. The 48 dormant unit tests: keep dormant through the split (recommended; `[lib] test = false`), then add a `cargo-unit`
  Nx target and a `unit` kind in `test_plan.rs` (`SCRIPTS`, `nx_drift` `TARGETS`) as its own follow-up, or enable them now and
  accept that their pass state is unknown.

## 10. Unverified

- No cargo command was run: every "moves with `pub` and path rewrites only" claim, the crate dependency edges, and every
  compile, link, disk and time effect of N crates come from a static scan of `mod` and `use crate::` paths (comments stripped)
  and from measured single-crate numbers, not from a split build. Items reached through methods, fields, associated consts or
  `use x as alias` are lower bounds (`e2e_tests`, `build_image`, `android_u2`, `android_input`, `vnc::descriptor` edges beyond
  the module edge).
- The module graph (71 of 96 modules in one component, 26 module edges and 36 references to cut) and the 12-area partition come
  from heuristic searches (12 annealing seeds, 11 reached the same value), not proven minima.
- The test-to-crate map is from file names and first argv words; per-test durations are not in any log (libtest prints only the
  60-second marker). The 29% share of `nx_cache` is a lower bound. W0 replaces both.
- The rerun percentages replay 435 past commits against my own file-to-crate and test-to-crate assignment (help files by
  command noun; `help_table.rs`, `main.rs`, `routes.rs` counted as `wf` only; every `Cargo.toml`, `tests/flow.rs` and support
  edit counted as global). Thirty days may not predict the next thirty.
- `^rustProduction` transitivity and whether `clis/pipeline/project.json` `inputs` replace or merge with the plugin's inferred
  ones: inferred from `nx.json`, `cargo-crates.js` and the `nx_cache` contract, not run (W0 and W6 prove them).
- Whether `pub(crate) use wf_kit::wf_err;` lets `crate::wf_err!` resolve.
- Whether a thin binary needs commands beyond the ones listed in section 4 (found only by running each crate's suite).
- Which other Dockerfiles, workflows or scripts copy a subset of `clis/`; whether the listener image builds without
  `browser_device_specs.json` (listed in its Dockerfile and `deploy.rs` extras, read only by `browser_launch.rs`).
- Whether the flow job really skips the Chromium tests (`have_browser` looks in `/opt/pw-browsers`; the job installs
  Playwright under `~/.cache/ms-playwright`).
- Whether `ship_eas::{latest_pointer,config_field,publish}` (used by `verify_tier3`, `e2e_tests`, `export_site`) all stay in
  `deploy` without a back edge from plan; `e2e_tests` device-run helpers are the only consumer in `wf`.
- Line counts of split modules (`verify_tier3`, `e2e_tests`, `deploy`, `test_publish`, `build_image`, `usage`, `help`) are
  estimates from function boundaries.
- The tree moved during the research (HEAD went from `ef90df980`, 152 tests, to `cde8a6d6e`, 159); counts are a snapshot of
  `cde8a6d6e` taken on 23 Oct, and a few per-file test counts of the researchers (check 7, vm 5, ship 14) are from the earlier tree.
