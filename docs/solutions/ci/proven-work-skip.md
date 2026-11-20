---
title: "Proven-work skip and head-only gating (design)"
date: 2026-11-20
category: ci
module: ci-pipeline
problem_type: workflow_issue
component: development_workflow
severity: medium
applies_when:
  - "Several pushes to dev in a row queue the same shards and runner minutes go to work that already passed"
  - "A job acts on a commit that is no longer the head of dev (a deploy, a publish, a test of a build nobody will use)"
  - "Choosing where a job writes a cache entry when several jobs restore the same key"
tags: [ci, queueing, proven-work, head-only, cache, runners]
---

## Decisions

Written before the code (issue #83, Q4 (4)); the verbs and the workflow edits follow it.

1. **Skip proven work, never re-run it.** A shard whose input fingerprint already passed in an earlier run of the same ref gets no runner: the plan job drops it from the job's matrix. Nx still replays what it can on the shards that do run; this removes the runner's fixed cost (checkout, setup, installs, cache restores) for a shard that would only replay.
2. **The pass marker is a cache key, not a state-ref push.** A job's token cannot push `refs/wf/state` (403 five times per stage, Q1), and parallel pushes race. A shard that passed saves a one-file Actions cache entry named `proven-<stage>-<fingerprint>`; the plan job lists the cache API for the key (`wf ci proven`). One writer per key: the entry cannot be overwritten, a second save of the same key is a warning, and `continue-on-error` keeps it from failing the job. An artifact would also do but dies with its run; a cache entry outlives runs and is what a later push has to read.
3. **The fingerprint is conservative.** The shard's key (`wf ci plan shard-keys`: its crates, the crates they depend on, the lock file) plus the git object ids of what every test reads (`.github`, `nx.json`, `.nxignore`, `mise.toml`, `rust-toolchain.toml`, `tooling`, `e2e`). Two kinds of shard are never proven because their key does not cover what they read: a package (its workspace dependencies are not in the key) and a crate with a `package.metadata.flow` table (inputs and binaries outside its directory). They always run. `core-tests` runs packages in every shard, so it is not wired; `integration-tests` and `cli-e2e-tests` are.
4. **Same ref only.** The cache API lists entries of every ref; a marker counts only when its `ref` is the run's own (`GITHUB_REF`), so a pull request's pass never proves a push to `dev`.
5. **A fresh run proves nothing and marks nothing.** `[cache:no]` (the weekly cache audit) runs every shard and saves no marker.
6. **Unreadable means run.** A failed API call, a missing token or an unknown plan runs every shard.
7. **Head-only gating.** A job whose work is only useful for the head of dev is skipped for a superseded commit (`needs.plan.outputs.is_head`): `deploy-build`, `eas-update`, `eas-update-e2e`, `deployment-verification-tests`, and `android-e2e-tests` (it loads the update `eas-update-e2e` publishes, and would fail without it). `is_head` is true off `dev`, so pull requests and dispatches run everything. The test stages are not head-only: a red superseded commit is still information.
8. **Not built: skipping a run a newer commit covers.** It waits for the hosted numbers of `CI_RUN_PROFILE` and `CI_CONCURRENCY` (parallelism-telemetry.md).

## Where each cache key has its one writer

| key | restored by | saved by |
| --- | --- | --- |
| `nx-<stage>-<shard>of<n>-...-<run>-<attempt>` | its shard | its shard (the run and attempt make it unique) |
| `sccache-<stage>-...-<week>-<locks>` | every shard of the stage | shard 1 of the stage (`save: matrix.shard == 1`); the browser and Android e2e stages now do the same (both shards used to save) |
| `metro-export-all-...` | `eas-update`, `eas-update-e2e` | `eas-update` on a green publish |
| `metro-export-web-...` | the browser e2e shards | browser shard 1 (it used to share the key of `eas-update`, so two writers raced for it and the web and native transform caches overwrote each other's slot) |
| `proven-<stage>-<fingerprint>` | `wf ci proven` (a listing, not a restore) | the shard that passed |

Each job restores each layer once.

## Also in this change

- `eas-update` needs only what its publish reads (`plan`, `wf-build`) and no longer waits for the test stages and `clis`; the pointer it hands over (`eas-pointer-production`) is written to the state ref by `eas-promote`, which needs the tests (`core-tests`, `integration-tests`, `cli-e2e-tests` success or skipped, and `clis`). A red run's update is published and phones can open it (as before), but the pointer the planner of the next push reads does not move, so the next push publishes again. Until a green run moves it, `wf ship eas --latest` and the planner read the pointer of the last green run, which can lag the update published to branch `dev` by one or more commits; `--latest` then names that older update, not the newest publish.
- The browser e2e job installs Chromium beside the web export (the export needs no browser and no apt) and joins the install before the harness build.

## Proof

`clis/pipeline/tests/flow/ci_proven.rs` (the verb against a fake Actions cache API), `ci_setup_cost.rs` and `pipeline.rs` (the workflow's structure), `ci_plan.rs` (the union of shards equals the plan's unit list). Hosted proof, over several pushes in a row: `CI_RUN_PROFILE` runner seconds per push before and after, and the `proven` list of `wf ci proven` in the plan job's log.

## What only a hosted run proves

The workflow tests in `clis/pipeline/tests/flow/ci_setup_cost.rs` read the YAML text, so they pin the wiring (needs, save flags), not GitHub's evaluation. These are exercised only by a hosted run and are not covered locally:

- `fromJSON(...)[matrix.shard]` compared with `''` after the proven step failed (the output is then empty, so the expression reads null rather than `''`; the step is `continue-on-error`, and every shard is meant to run in that case).
- `cache/save` on a key that already exists (the `continue-on-error` path of the marker and the sccache and metro saves).
- The Chromium rc-file join.
- `eas-promote` writing the pointer: its loop runs over `eas-pointer/latest-*.json` with no `nullglob`, so an empty artifact directory passes the literal pattern to `wf bucket state write`, which fails on the missing file (loud, not a silent pass).
