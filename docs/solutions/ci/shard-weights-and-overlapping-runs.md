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

`clis/pipeline` deals the flow and integration tests to the caller's `shards` machines by unit key, not round-robin. A unit is a package, a crate (`crate-<crate>`) or, for integration and e2e groups, `crate-<crate>/<group>`; its weight is in `clis/pipeline/data/shard-weights.json` (a default per stage, an entry for each heavy unit). The units are dealt longest first to the lightest shard (`assign` in `test_plan.rs`), so the same commit always puts the same unit on the same shard and a shard's Nx cache key and CI timing row mean the same thing from run to run. Change the weights when a unit's measured time moves; the tests in `shard_tests` pin the dealing.

## Overlapping runs

`ci-hosted.yml` groups runs per commit (`ci-hosted-<sha>`), so a push never waits for an older run and a red shard is fixed and re-run at once; no run is cancelled, and every run is measured to its end. Per-job groups still serialise what must not overlap (the deploy per app). The `Is this commit the head of dev` step of the `plan` job in `pipeline.yml` (output `is_head`; it was a job of its own, which took a runner slot for 3 s) gates the deploy and the production EAS publish on the commit still being the head of `dev`, so a slow older run never puts older code over newer.

## The e2e jobs are per commit too

`e2e-tests.yml` used one concurrency group per platform for every run. GitHub keeps one running and one pending job per group and cancels the older pending one, so a newer push silently cancelled the browser and Android jobs of the run before it (3a110f34, no job red). The group now carries the commit, so overlapping runs each finish their own e2e jobs.

## The e2e jobs do not wait for the integration tests

`browser-e2e-tests` runs against a web export built on its own runner and `android-e2e-tests` against its own backends, so neither reads the deploy or the production EAS Update, which both wait for every stage including the slowest integration shard (about 800 s). They now need `plan`, `core-tests`, `clis` and `cli-e2e-tests`; Android also needs `eas-update-e2e`, the job that publishes the branch `e2e` update and hands its pointer over, split out of `eas-update` (which keeps the production publish gated on the tests).

## Integration shards queue per crate set and inputs

The concurrency group of an integration shard is `hosted-integration-<caller workflow>-<key>` (the caller is in it because two workflows of one commit plan the same keys, and one cancelled the other's pending shard, run 44708906862's sibling on 9d133fb2), the key being `wf ci plan shard-keys` of what that shard runs: the crates it holds and a fingerprint of what they read (the git object ids at the commit of their directories, of the directories of the crates they depend on, and of the lock file). Two commits that plan the same crates with the same inputs share a group: the later one waits for the earlier one and replays its cached result, and a third commit replaces the second's pending job (owner, 19 Nov: desired). A fix to a crate changes its fingerprint, so it is another group and starts at once beside the earlier run; a change to an unrelated crate leaves the key alone. A failed task is not cached, so a queued commit behind a red one runs the task again. The key follows the weighted split of that commit's plan, so a plan that moves a crate to another shard changes the key.

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

## Six workflows on one push (owner, 19 Nov)

A push to `dev` starts six runs of the one pipeline graph, each for a stage (`stages` input of `pipeline.yml`) and, for the core stage, an area (`area` input, `wf ci plan tests --area`): `mobile_core+lint+pre-release-testing` (`ci-hosted-core-mobile.yml`: the app and the workspace root), `cli_core+lint+pre-release-testing` (`ci-hosted-core-cli.yml`: `clis/`) and `backend_core+lint+pre-release-testing` (`ci-hosted.yml`: everything else) run the flow, lint, typecheck and unit tests of their area; `cli_build+publish+release-testing` (`ci-hosted-cli-publish.yml`) runs the cli e2e tests and the wf build released as `wf-latest`; `backend_build+deploy+release-testing` (`ci-hosted-service-deploy.yml`) the image builds, the integration tests (which belong with the images they run) and the deploy legs with their verification; `mobile_build+publish+release-testing` (`ci-hosted-mobile-e2e.yml`) the production EAS Update, the Android e2e update and the browser and Android suites, side by side. Each has its own concurrency group, its own pull request comment (a `label` of its own) and its own result, and the shared jobs (`wf-build`, `preflight`, `plan`, `report`) run in each. None waits for another. `--base last-green` is read per workflow (`GITHUB_WORKFLOW_REF`), so a workflow's changed set is measured from its own last green. `cli-e2e-tests` is the job the owner called both the crate e2e and the cli e2e; moving it to `deploy` is one `if:` line. `wf vm ops cancel-run` and `wf ci audit` look at `ci-hosted.yml` only (now the backend core), which is the file name they were built on.

## The 20-job cap and how many shards (owner, 19 Nov)

GitHub-hosted runners are free for a public repository but the account runs at most 20 jobs at once, whatever the workflow; the rest queue. The first six-workflow push ran 68 jobs and jobs waited up to 267 s for a slot (the push before it, 736 s), so a shard is not free: each repeats 1 to 3 minutes of setup and takes a slot. The callers therefore deal each stage over few shards (3 for the core areas and the cli publish, 4 for the integration tests, whose heaviest unit is 542 s; the repository variable `HOSTED_SHARDS` is no longer read: each caller states its own number in its `with:`) and the Android e2e over 2 devices. Measure the effect with `CI_JOBS.QUEUED` before adding a shard.

## Four cores per runner, fewer runners (owner, 20 Nov)

A hosted runner is 4 vCPU and 16 GB, and the account's 20 slots are shared by all six callers, so a slot that holds two cores' worth of work is half wasted. Two changes, stage names untouched (the test plan, `shard-weights.json`, the run bundles and the last-green logic still say `flow-tests` and `typecheck-lint-unit-tests`; only job names changed):

- **`core-tests` is one job per shard.** It replaces the separate `flow-tests` and `typecheck-lint-unit-tests` jobs. Shard `i/N` starts `wf test run --stage flow-tests --parallel 2 --shard i/N` and `wf test run --stage typecheck-lint-unit-tests --parallel 2 --shard (N+1-i)/N` as two background processes (2 + 2 Nx tasks = 4 cores), waits for both and fails if either failed. Both stages use the caller's one `shards` count (`plan` step `shards`), so a caller's core run goes from 2N shard jobs to N. The typecheck stage takes the mirrored shard because the weighted dealing puts the heaviest units on shard 1 in both stages: mirrored, the machine with the heaviest flow units has the lightest typecheck units. Each stage is still dealt by its own weights. The run bundle is still published as `test-runs-flow-tests-<i>`. Output lines carry a `[flow-tests]` or `[typecheck-lint-unit-tests]` prefix. The job's Nx cache key is `core-tests-<i>of<N>` (the first run after the change starts cold), its sccache key stays `flow-tests`, its sampler and ledger name is `core-tests-<i>` (the ledger rows keep the stage as `JOB`, `nx_ledger::record`, so `wf ci plan shard-weights` still reads them).
- **`plan` also runs the preflight and the head check.** The `preflight` job (58 s of setup for a check that waits on the network) and the `head` job (3 s, a runner slot all the same) are steps of `plan`: `wf ci preflight` starts in the background right after the setup, the plan steps run beside it, and `Collect the preflight` prints its output at the end. That step keeps the old `continue-on-error`, so a red preflight is an error annotation and a line in the log, not a stop; remove that line to make it stop the run (every job needs `plan`). `plan` also calls `wf ci concurrency --phase start` (the start of the run's concurrency record) and the report job runs `wf ci resources` and `wf ci concurrency` after `Timings`.

Runner slots at peak per caller, with the core callers at 3 shards:

| Caller | Before | After |
| --- | --- | --- |
| core mobile, core cli, core backend (each) | 6 (3 flow + 3 typecheck), 2 earlier (preflight + plan), 4 at the e2e wait | 3 (`core-tests`), 1 (`plan`) |
| cli publish | `wf-build` 1, `preflight` + `plan` 2, 3 cli-e2e, `clis` 1 | 1, `plan` 1, 3 cli-e2e, `clis` 1 |
| service deploy | `head` 1 beside `wf-build`, 4 integration, deploy legs | no `head` slot; the rest as before |
| mobile e2e | `head`, `preflight` and `plan` | `plan` only |

Every run also saves `report` (one slot) and the 3 s `trigger` job per caller. The three core callers together peak at 9 test runners instead of 18, which leaves the other callers room under the 20-slot cap.

## Several pushes in a row (owner, 20 Nov)

Every sharded test job (`core-tests`, `integration-tests`, `cli-e2e-tests`) has the concurrency group `hosted-<job>-<key>`, the key being the shard's input fingerprint (`wf ci plan shard-keys`; `core-tests` combines its flow and typecheck shard keys), `-fresh` for a run with the caches off. Identical work from two pushes minutes apart waits as a pending job, which holds no runner and no slot of the cap, and replays from the Nx cache; a fix push changes the inputs, so it starts at once; nothing running is cancelled. A pending job that a newer one replaces ends `cancelled`, and the deploy, clis and eas-update gates read `cancelled` as not passed (`success` or `skipped` only; `!= 'failure'` let it through). `install-wf` caches the built `wf` binaries by the `clis/` tree (exact key), so a commit that leaves `clis/` alone builds nothing in `wf-build`. Not done, on purpose: a marker that ends a released waiter in seconds instead of an Nx replay, and skipping a run a newer commit covers; both wait for the numbers of `CI_RUN_PROFILE` and `CI_CONCURRENCY` (docs/solutions/ci/parallelism-telemetry.md). Push a batch of commits once; push separately only a fix to a red run.


## The e2e stages and the legs probe (owner, 20 Nov)

Measured on 13 recent runs (`wf test results`), per test in seconds: voice-edit 150, tamil-writing 127, editor-typing 90, local-run 38, review-sheet-keyboard 22, stubbed-run-local 6, pyodide-spike 5 on the device; on the browser voice-edit 130, editor-typing 90, tamil-writing 81, app-routes 16, pyodide-spike 5 (local-run and review-sheet-keyboard have no browser time yet and weigh the stage default of 60).

- **`android-e2e-tests` weights are the measured ones** (default 40). The old weights (editor-typing 400, tamil-writing 240) put 140 s of tests on shard 1 and 343 s on shard 2; the deal is now 221 s (voice-edit, local-run, review-sheet-keyboard, stubbed-run-local, pyodide-spike) against 217 s (tamil-writing, editor-typing). A test missing from the table falls to the default, so a new test needs its measured time added here.
- **`browser-e2e-tests` is sharded in two** like Android (`shard`, `shards: 2` and a matrix in `pipeline.yml`; `e2e-tests.yml` already passed `--shard i/n` and put the shard in the job name, concurrency group and bundle name). The stage has its own weights; the deal is voice-edit, local-run, app-routes, pyodide-spike against editor-typing, tamil-writing, review-sheet-keyboard (about 211 and 231 weighted seconds). Each shard repeats the runner's setup, backends and export (about 4 minutes of runner time), so the stage costs one more job slot and about 380 runner-seconds more; the job's wall time should fall from about 745 s to about 580 s. Revert by removing the matrix keys and `shards`.
- **`plan` does not ask the VM in a run with no deploy.** `wf ci plan legs --no-lookup` asks no service for its running commit (each reads as unknown, so every leg lists as changed, exactly what a service that does not answer gave before); `pipeline.yml` passes it when `inputs.deploy` is false, which saves the roughly 45 s of timed-out probes in the mobile and backend build workflows. The update's decision still comes from the latest-update pointer. A run with `deploy` true probes as before.
