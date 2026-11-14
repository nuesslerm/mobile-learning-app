---
title: Cache bench, what each CI cache layer is worth
---

# Cache bench

`.github/workflows/cache-bench.yml` measures, on GitHub-hosted runners, how many seconds each cache layer of the pipeline saves, so the 10 GB the repository gets from the GitHub Actions cache (least recently used entries evicted first) goes to the layers that pay. It needs no secret and no tailnet, and it exists to be run again when a blob store returns (below).

## What it measures

Every job runs the pipeline's own command cold, saves the layer with `actions/cache/save` under `bench-<run id>-<layer>-...`, wipes the local copy, restores it with `actions/cache/restore` (timed, so the download and the unpack count), runs the command warm, and prints one line per measurement. A job deletes its caches when it is done; the `cleanup` job deletes any that are left (every key that starts with `bench-<run id>-`) and prints the table of valid rows sorted by `net_s`; baselines and rows whose restore missed are listed apart.

| Job | Cold | Cached layer | Warm |
|---|---|---|---|
| `submodules` | `git submodule update --init --recommend-shallow --depth 1 references/orca references/t3code` | `.git/modules/references` | the same command over the restored module directories |
| `pnpm` | `pnpm install --frozen-lockfile` from nothing | variant `store`: `pnpm store path`; variant `store+node_modules`: the store and every workspace `node_modules` | `pnpm install --frozen-lockfile` |
| `playwright` (control) | the `install-test-deps` action with `playwright: true` | `~/.cache/ms-playwright` | the same action, which finds its marker file in the restored directory |
| `toolchain` (control) | `rustup toolchain install --profile minimal` | the pinned toolchain under `~/.rustup`, its update hash and `~/.cargo/bin` | `cargo --version`, and the install again only if it fails (`installed_again`) |
| `cargo-fetch` | `cargo fetch --locked` for every Cargo workspace | `~/.cargo/registry` and `~/.cargo/git` | `cargo fetch --locked` |
| `rust-compile (baseline)` | `wf test run --stage flow-tests --plan <plan> --parallel 2 --shard 5/6 --fresh`, nothing cached | none | none: the cold run is the baseline |
| `rust-compile (sccache)` | the same, with `RUSTC_WRAPPER=sccache`, `CARGO_INCREMENTAL=0`, `SCCACHE_DIR`, `SCCACHE_CACHE_SIZE=3G` | `SCCACHE_DIR` | the same command with an empty target directory after a one-line comment appended to a crate source; `sccache --show-stats` hits and misses in the line |
| `rust-compile (target)` | the same, nothing cached | the cargo target directory (a fixed `CARGO_TARGET_DIR`, not saved over 6000 MB) | the same command after every tracked file was touched (a new job's checkout is newer than the restored artifacts) and a comment was appended to a crate source |
| `nx` | `wf test run --stage typecheck-lint-unit-tests` and `--stage flow-tests` over a plan of three TypeScript packages (`@lib/logging`, `@lib/app-config`, `@lib/gesture-geometry`: typecheck, lint, unit) and the `telemetry` crate's `cargo-flow` (telemetry has its own `[workspace]`, so its source is found with `cargo metadata` on `clis/telemetry/Cargo.toml` and its target directory `clis/telemetry/target` is wiped) | `~/.nx/*/cache` and `~/.nx/*/databases` | the same commands after a comment appended to one TypeScript file and one crate source (variant `narrow-change`), and again unchanged (variant `identical-rerun`); the replayed and rerun task counts are in the line |
| `metro` | `nx run mla-tamagui:typecheck --skip-nx-cache` and `nx run mla-tamagui:export:web --skip-nx-cache`, backends at `http://127.0.0.1:1` | `frontend/mla-tamagui/node_modules/.cache/app-build` (Metro's transform cache and tsc's build info) | the same commands after a comment appended to one app source file |

Details that decide what a number means:

- The compile jobs work on the crate entries of the flow plan only (`wf test plan --all --json`, filtered to entries with a `crate`), dealt into six shards by the same round robin over sorted names that `wf test run --shard` uses; shard 5 of 6 is `cli-http`, `offload`, `orca-map-hosted`, `orca-serve`, `secrets` and `wf-harness`. The flow tests of the TypeScript packages need browsers and other tools and are not part of it. The crate that gets the narrow change is the one of the shard that the fewest workspace crates depend on (`leaf_crate_source`, which fails the job with an `::error::` when it finds no source file).
- `--fresh` is how the Nx cache is off in the compile jobs: it sets `NX_SKIP_NX_CACHE=true` and starts no build-cache shim. The `nx` job leaves it off. `WF_BLOB_CACHE=off` is set for every job, as in `pipeline.yml`, so no job reaches a bucket.
- `CARGO_INCREMENTAL=0` and a fixed `CARGO_TARGET_DIR` are set in all three compile variants, so they compile the same work; the pipeline's own default (incremental) is not what is compared.
- The narrow change is one comment line appended to a source file in the job's workspace. It is never committed. A comment changes the file's hash and mtime but not what the compiler produces, so it is the gentlest realistic push; a real push that changes code reruns more.
- `playwright` is a control: its marker file also skips `--with-deps` (apt). The cold step already installed those packages on the same runner, so the number does not show that cost on a fresh machine.

## How to run it

Agents cannot dispatch workflows, and GitHub generally refuses to dispatch a NEW `workflow_dispatch` workflow that is not on the default branch (`main`) while `dev` is not the default. So the run starts from a push: an EMPTY commit to `dev` whose message starts with `[cache-bench]`, through the `push` trigger of `cache-bench.yml` (`on: push: branches: [dev]`). Every job is guarded by `github.ref == 'refs/heads/dev' && (github.event_name == 'workflow_dispatch' || startsWith(github.event.head_commit.message, '[cache-bench]'))`, so a push without the tag skips all of them at once.

```
git commit --allow-empty -m "[cache-bench] measure the cache layers"
git fetch origin dev && git rebase origin/dev && git push origin dev
```

`ci-hosted.yml` starts no pipeline run for an empty commit, so the bench does not displace a queued run. The cost of the push trigger: every normal push to `dev` leaves one skipped bench job on the checks list. That is accepted until `main` has the file; then the `workflow_dispatch` route (a `[cache-bench]` entry in `scheduled-triggers.yml` that dispatches `cache-bench.yml` with `message` and `sha`) can be used and the push trigger removed. `scheduled-triggers.yml` does not list the tag today, so a tagged commit starts exactly one run.

## How to read the BENCH lines

Each job prints lines of this form to its log, and the `cleanup` job prints all of them again, best first, with a table in its job summary:

```
BENCH {"layer":"pnpm","variant":"store","cold_s":41.2,"restore_s":9.8,"warm_s":14.1,"cache_mb":1300,"saved_mb_compressed":540,"net_s":17.3,"restore_hit":"true"}
```

- `cold_s`: the command with an empty cache. `restore_s`: the restore step, download and unpack included. `warm_s`: the command with the cache restored (and after the narrow change where it applies).
- A row is valid only when `restore_hit` is `true`. A row whose restore missed the key has `net_s` null, `valid` `NO` in the table and ranks last: it timed a cold run twice. The baseline row (`rust-compile no-cache`) has no `net_s` and is printed in its own `baselines` list, not ranked.
- `net_s = cold_s - (restore_s + warm_s)`: the seconds a job gains by restoring the layer. A negative number means the layer costs more than it saves. Weigh it by how many jobs of a pipeline run use the layer: the per-job setup is paid in about 25 jobs, a shard's compile in one.
- `cache_mb` is the size on disk after the restore (`du`); `saved_mb_compressed` is what the cache service holds, from `gh cache list`. The second number is what counts against the 10 GB.
- A layer's value per gigabyte is `net_s` times the jobs that use it, divided by `saved_mb_compressed`: spend the quota in that order.
- Extra fields: `restore_hit` (the restore found the exact key; anything but `true` voids the line), `rc`, `rc_cold` and `rc_warm` (the exit code of `wf test run`: a red test run still measures the compile, a failed Nx task is not cached and so not replayed), `sccache_cold` and `sccache_warm` (requests, hits, misses), `tasks_cold` and `tasks_warm` (Nx `replayed`, `rerun`, `tasks`, `entries`), `saved` (`false`: the target directory was over the size limit and was not saved).

A job that fails before its `BENCH` line has no result; read its log. The jobs are independent, each has a 25 minute timeout, and the compile jobs finish in about 15.

## Re-run it when a blob store returns

`pipeline.yml` runs with `WF_BLOB_CACHE: "off"` because the Oracle bucket is unreachable (`docs/solutions/ci/cache-layers.md`). When a store is back, run the bench again and compare the backends: the `restore_s` of a blob restore (`wf cache restore|save pnpm|metro`, the Nx and sccache shim) against the `restore_s` here, with the same `cold_s`. A blob store can also supplement the Actions cache: layers whose compressed size is large for what they save belong in the blob store, small layers with a high `net_s` stay in `actions/cache`. To measure a blob backend, add a second variant to each job next to the `actions/cache` steps (save, wipe, restore, warm) and keep the `emit` call and its field names; the table needs no change.
