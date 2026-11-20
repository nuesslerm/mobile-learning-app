---
title: "Deterministic shard weights and overlapping runs"
date: 2026-11-19
category: ci
module: ci-pipeline
problem_type: workflow_issue
component: development_workflow
severity: medium
applies_when:
  - "A flow or integration shard is red and the fix should re-run it without waiting for the other shards or an older run"
  - "Changing how tests are dealt to the hosted shards, or the concurrency of ci-hosted.yml"
tags: [ci, shards, concurrency, nx-cache]
---

## The split

`clis/pipeline` deals the flow and integration tests to the `HOSTED_SHARDS` machines by unit key, not round-robin. A unit is a package, a crate (`crate-<crate>`) or, for integration and e2e groups, `crate-<crate>/<group>`; its weight is in `clis/pipeline/data/shard-weights.json` (a default per stage, an entry for each heavy unit). The units are dealt longest first to the lightest shard (`assign` in `test_plan.rs`), so the same commit always puts the same unit on the same shard and a shard's Nx cache key and CI timing row mean the same thing from run to run. Change the weights when a unit's measured time moves; the tests in `shard_tests` pin the dealing.

## Overlapping runs

`ci-hosted.yml` groups runs per commit (`ci-hosted-<sha>`), so a push never waits for an older run and a red shard is fixed and re-run at once; no run is cancelled, and every run is measured to its end. Per-job groups still serialise what must not overlap (the deploy per app). The `head` job in `pipeline.yml` gates the deploy and the production EAS publish on the commit still being the head of `dev`, so a slow older run never puts older code over newer.

## The e2e jobs are per commit too

`e2e-tests.yml` used one concurrency group per platform for every run. GitHub keeps one running and one pending job per group and cancels the older pending one, so a newer push silently cancelled the browser and Android jobs of the run before it (3a110f34, no job red). The group now carries the commit, so overlapping runs each finish their own e2e jobs.

## The e2e jobs do not wait for the integration tests

`browser-e2e-tests` runs against a web export built on its own runner and `android-e2e-tests` against its own backends, so neither reads the deploy or the production EAS Update, which both wait for every stage including the slowest integration shard (about 800 s). They now need `plan`, `flow-tests`, `typecheck-lint-unit-tests`, `clis` and `cli-e2e-tests`; Android also needs `eas-update-e2e`, the job that publishes the branch `e2e` update and hands its pointer over, split out of `eas-update` (which keeps the production publish gated on the tests).

## Integration shards queue per crate set and inputs

The concurrency group of an integration shard is `hosted-integration-<key>`, the key being `wf ci plan shard-keys` of what that shard runs: the crates it holds and a fingerprint of what they read (the git object ids at the commit of their directories, of the directories of the crates they depend on, and of the lock file). Two commits that plan the same crates with the same inputs share a group: the later one waits for the earlier one and replays its cached result, and a third commit replaces the second's pending job (owner, 19 Nov: desired). A fix to a crate changes its fingerprint, so it is another group and starts at once beside the earlier run; a change to an unrelated crate leaves the key alone. A failed task is not cached, so a queued commit behind a red one runs the task again. The key follows the weighted split of that commit's plan, so a plan that moves a crate to another shard changes the key.

## Checking the queue after some days of use

The sealed CI database answers these, with `wf ci db query` (`CI_JOBS` holds every finished job of a run, cancelled-while-pending ones included; `NX_TASKS` every task; `CI_METRICS` the same gauge readings Better Stack receives):

```sql
-- work done twice: the same task and input hash that ran in more than one run (should trend to zero)
SELECT TASK, HASH, COUNT(DISTINCT RUN_ID) AS runs FROM NX_TASKS WHERE OUTCOME = 'ran' GROUP BY TASK, HASH HAVING runs > 1;
-- an overlap inside a group: two jobs of one group running at the same time (should be none)
SELECT a.GROUP_KEY, a.RUN_ID, b.RUN_ID FROM CI_JOBS a JOIN CI_JOBS b ON a.GROUP_KEY = b.GROUP_KEY AND a.RUN_ID < b.RUN_ID
  WHERE a.STARTED > 0 AND b.STARTED > 0 AND a.STARTED < b.STARTED + b.SECS AND b.STARTED < a.STARTED + a.SECS;
-- a job killed while running (a cancelled job that had started; only pending jobs should be replaced)
SELECT RUN_ID, JOB FROM CI_JOBS WHERE CONCLUSION = 'cancelled' AND STARTED > 0;
-- how long a shard waited, by whether it was replaced (a fix push should wait a few seconds)
SELECT CONCLUSION, COUNT(*), AVG(QUEUED), MAX(QUEUED) FROM CI_JOBS WHERE KIND = 'integration-tests' GROUP BY CONCLUSION;
-- why jobs failed, as the run saw it
SELECT RUN_ID, JOB, STEP, EXCERPT FROM CI_JOBS WHERE CONCLUSION IN ('failure', 'timed_out') ORDER BY TS DESC LIMIT 20;
```

A job that goes red also sends the Better Stack log line `ci.job.failed` (run, job, step, sha, and `annotations`: how many error annotations the API showed for the running job, which tells whether they are readable live) and posts one JSON line to `CI_FAILURE_HOOK_URL` when the vault holds one.

## Three workflows on one push (owner, 19 Nov)

A push to `dev` starts three runs of the one pipeline graph, each for a stage of its own (`stages` input of `pipeline.yml`): `ci-hosted.yml` (`core`: flow, unit, cli-e2e, the ships and the deploy), `ci-hosted-integration.yml` (`integration`) and `ci-hosted-e2e.yml` (`e2e`: the browser and Android suites and the Android EAS Update). Each has its own concurrency group, its own pull request comment (a `label` of its own) and its own result, and the shared jobs (`wf-build`, `preflight`, `plan`, `report`) run in each. The deploy and the production EAS Update no longer wait for the integration tests. `wf vm ops cancel-run`, `wf ci audit` and the `last-green` base look at `ci-hosted.yml` only, so "green" for them is the core stage.
