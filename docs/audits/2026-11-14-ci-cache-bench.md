---
title: CI cache bench, 14 Nov 2026
---

# Which CI cache is worth its gigabytes

Test type: live, on GitHub-hosted runners (`cache-bench.yml`, run 43558013495, commit `bf157f1`, n=1 per layer). Every row is the pipeline's own command, cold, then saved with `actions/cache`, wiped, restored (timed), run warm. `net_s` = cold − (restore + warm). The repository gets 10 GB of GitHub Actions cache; entries are immutable, evicted least recently used first, and `dev` is not the default branch, so a cache made on `dev` is read by `dev` runs only.

## The ranking

| # | Layer | Seconds saved per use | Cache on disk (compressed) | Seconds per MB | Verdict |
|---|---|---|---|---|---|
| 1 | metro (`app-build`) | 69.8 | 17 MB | 4.1 | keep |
| 2 | sccache (`SCCACHE_DIR`) | 96.1 | 268 MB | 0.36 | add |
| 3 | nx (`~/.nx/*/cache`) | 38.2 on an identical rerun, 8.0 on a narrow change | 1 MB | 38 | keep, it is free |
| 4 | rust target directory | 85.9 | 348 MB | 0.25 | do not use, sccache beats it |
| 5 | playwright browsers | 20.1 (upper bound) | 269 MB | 0.07 | not yet: the control skipped `--with-deps` |
| 6 | pnpm store + node_modules | 17.0 | 736 MB | 0.02 | off |
| 7 | pnpm store | 12.9 | 705 MB | 0.02 | off |
| 8 | rustup toolchain | 4.3 | 202 MB | 0.02 | drop |
| 9 | cargo registry and git | 0.5 | 146 MB | 0.003 | drop |
| – | submodules | not measured: the save failed (below) | 489 MB raw | | cached by pinned commits; measured on the pipeline |

Rust compile baseline with nothing cached: 172.1 s. sccache cold 182.2 s (a 10 s tax on a miss), warm 84.6 s with 444 hits and 1 miss out of 555 requests. Target directory cold 195.3 s, warm 105.8 s.

## What this says

- The 10 GB is not the constraint. Everything worth keeping together is about 0.6 GB compressed (sccache 268, playwright 269, metro 17, nx 1). What can fill 10 GB is key churn: a key that changes every run (nx saves one per run) piles up until the least recently used ones are evicted. Keys below are chosen so the steady state stays under 1.5 GB.
- sccache beats the target directory on both axes (more seconds, fewer megabytes) and it survives a changed source file, which the target directory does less well. Cost: 10 s on a full miss.
- pnpm is the surprise. The bench says 13 to 17 s, but the real pipeline job logs (runs 43552129531, 43552757919: restore 14 s plus `pnpm install` 35 s with 1476 of 1571 packages reused) show no gain over a cold install (28 to 56 s). A 705 MB restore on every one of about 25 jobs buys nothing measurable: switch it off.
- nx is tiny (1 MB) because the compile jobs run `--fresh`; what it replays is the TypeScript and light Rust tasks. It is cheap enough to keep, and the nightly audit's 90 hard `should-have-hit` findings (cargo tasks of 300 to 480 s that the Nx cache held but did not serve, run 43404231153) were measured before this layer existed.
- Submodules cost 58 s on a cold clone in each of 12 jobs (about 700 runner seconds), the largest uncached cost left in `install-test-deps`. The bench's save failed (`tar: file changed as we read it`, a git temp pack written by an auto `gc`), so the row is marked invalid. The bench is fixed (`gc.auto=0`); the next run measures it. Independent of the cache, most jobs may not need `references/` at all.

## The pipeline, end to end

| Run | Commit | Wall | State |
|---|---|---|---|
| 43550451832 | `7370298` | 24m 24s | cold caches; only eas-update red |
| 43552129531 | `ebbb61d` | 21m 08s | first warm; only eas-update red |
| 43552757919 | `14d5902` | 15m 50s | eas-update reached the publish; red on the missing bucket |
| 43556008053 | `afed545` | 15m 34s | green: 29 passed, 6 skipped |

The end-to-end drop is not attributable to one layer: the long flow shard went from 16m 26s to 6m 43s, mostly Rust compile and Nx replay variance between runs. Use the bench, not these rows, to compare layers.

## Decision

1. Keep metro, nx. Add sccache (`SCCACHE_DIR`, local disk): `wf-build`, and `flow-tests`, `integration-tests` and `cli-e2e-tests` (shard 1 saves). Turn pnpm, toolchain and cargo-fetch off. Playwright waits for a fresh-runner measurement (restore plus `playwright install-deps`).
2. One switch decides the backend, so a blob store can replace or supplement the Actions cache later (below).
3. Cache the reference trees' git objects (`install-test-deps`, keyed on the two pinned commits): a flow shard spends 56 s of its 105 s of `install-test-deps` on the serial clone (run 43559002207, job 132339407616; Playwright 25 s, apt 18 s, pnpm 27 s). The layer is read off the pipeline (the `references` summary line and the step seconds) and by the next bench run.
4. Re-run the bench whenever the pipeline changes shape, or when the blob store is back (`CACHE_BACKENDS=blob` rows against the same commands).

## The switch

`CACHE_BACKENDS` (repository variable, default `gha`) takes `gha`, `blob` or `gha,blob`; `pipeline.yml` exports it and sets `WF_BLOB_CACHE` from it, and every cache action reads it. `gha` uses `actions/cache` as in the table above. `blob` uses what is already there: `wf cache` and the build-cache shim of `wf test run`, over the store lists `BUILD_CACHE_READ_STORES` and `BUILD_CACHE_WRITE_STORES` (reads take the first hit, writes go to every store). With both, the Actions-cache steps run and the blob store is also read and written; sccache is the exception (one backend: the blob shim owns the compiler wrapper while it is on). Moving to R2 or Hetzner Object Storage is the variable and two store URLs, with no workflow edit. `CACHE_PNPM=on` brings the pnpm store back. See `docs/solutions/ci/cache-layers.md`.
