---
title: "Measuring hosted runner minutes: unit U0.3 of the CI-scoped vault plan"
date: 2026-11-08
category: ci
module: ci-vault-scopes
problem_type: workflow_issue
component: development_workflow
severity: medium
applies_when:
  - "Deciding whether a CI job can move to a GitHub-hosted runner (docs/plans/ci-scoped-vault-access/plan.md)"
  - "Choosing between ubuntu-latest (x64) and ubuntu-24.04-arm for a hosted job"
tags: [ci, runners, billing, minutes, arm64]
---

# Measuring hosted runner minutes

Measured 9 Nov 2026 (two x64 runs, one arm64 run; the second run of each leg was not needed because the stop rule never came close). The workflow is `.github/workflows/hosted-minutes.yml` (`workflow_dispatch`, or an empty marker commit to `dev` `[hosted-minutes x64]` / `[hosted-minutes arm64]`; no secret, no vault token, no tailnet), one job that runs `cargo test --locked --workspace` in `libraries/` (the `clis` workspace needs the private registry behind the tailnet, unreachable from a hosted runner before Phase 5; first attempt 9 Nov failed in 45 s on `Could not resolve host: libs-proxy`) and prints the runner name, architecture and elapsed seconds.

## Steps

1. Read the plan's budget: the private repo has 2000 free Linux minutes a month at 1x, and `ubuntu-24.04-arm` is not free for private repos (plan finding 6). Record the month's used minutes from the repo's billing page before starting.
2. Dispatch the workflow on `dev` with `runner = ubuntu-latest`: `gh workflow run hosted-minutes.yml --ref dev -f runner=ubuntu-latest`.
3. When it ends, read the job's elapsed seconds from the step summary and the billed minutes from the run's usage (`gh api repos/{owner}/{repo}/actions/runs/<id>/timing`). Billing rounds each job up to a whole minute.
4. Repeat with `runner = ubuntu-24.04-arm`. If the arm label is refused for the private repo, record that and stop that leg.
5. Repeat each runner once more to see the cold-versus-warm difference (the first run has no cargo cache; this workflow caches nothing).
6. Fill the table, extrapolate one push of the CI pipeline from the cheapest job's seconds, and write the decision in the plan's U3.5.

## Results

| Runner | Run | Elapsed s | Billed min | Multiplier | Notes |
| --- | --- | --- | --- | --- | --- |
| ubuntu-latest (x64) | 1 | 45 | 1 | 1x | failed on `libs-proxy` (the `clis` workspace needs the tailnet registry); run 42530229004 |
| ubuntu-latest (x64) | 2 | 31 | 1 | 1x | `libraries` workspace tests, green; run 42534418019 |
| ubuntu-24.04-arm | 1 | 28 | 1 | 1x | same job, green; run 42536837846; billed under the sku `Actions Linux ARM` |
| ubuntu-24.04-arm | 2 | not run | not run | not run | one run per leg was enough for a decision |

Source of the billed minutes: the billing usage API read from agent-shell with the PAT (`wf vm exec`). Before: 3 `Actions Linux` minutes for this repo in November. After the three runs: 5 `Actions Linux` plus 1 `Actions Linux ARM`, net amount $0. Each job bills one whole minute whatever its seconds, so these runs prove rounding and sku, not the per-minute multiplier of a long arm job. The VM runner's hours of CI the same day did not move the counter (the account-wide November total is 14 minutes): self-hosted minutes are not counted today.

Free minutes left after the runs: about 1986 of 2000 for the account (vectorbt used 8, this repo 6). Cost of one CI push if every open job ran hosted: not measurable yet. Only a job that needs no private registry can run on a hosted runner before the Phase 5 tailnet join (a hosted runner cannot resolve `libs-proxy`), and the open jobs of the old `deploy.yml` built the `clis` workspace. Re-measure after the join.

Decision (U3.5): the stop rule did not trigger (no run above 15 minutes, 3 minutes of 2000 used, arm64 not refused, billed at 1 minute for a sub-minute job). The VM runner was removed on 2026-11-13: arm64 builds run on the hosted arm64 runner (`ubuntu-24.04-arm`) and every other job on x64 hosted.

## The hosted twin of the CI, against the VM CI (9 Nov 2026)

`.github/workflows/ci-hosted.yml` ("CI (GitHub-hosted)") runs on every push to `dev` next to the VM CI: it builds `wf` once, then plan, typecheck-lint-unit-tests, flow-tests and integration-tests on `ubuntu-latest` (x64, 2 vCPU) through the tailnet, with its own pull request comment. It deploys nothing. `HOSTED_CI=off` (repo variable) switches it off.

| Job | Hosted run 2 (cold, x64) | VM CI run 1237 (Nx cache, arm64) | Hosted billed (rounded up per job) |
| --- | --- | --- | --- |
| wf build | 215 s | 8 s (`clis`, incremental) | 4 min |
| plan | 58 s | 40 s | 1 min |
| typecheck-lint-unit-tests | 139 s | 36 s | 3 min |
| flow-tests | 275 s | 43 s | 5 min |
| integration-tests | 222 s | skipped | 4 min |
| report | 12 s | n/a | 1 min |
| Whole run | about 15 min of job time | about 2 min | 18 min |

What it says: the VM CI is fast because its Nx cache replays what did not change (the first run of the twin on a workflow-only push took 6 min 10 s against 3 min 30 s). The hosted runner is x64 and the cache keys differ by CPU, so every hosted job runs cold and nothing replays; it also ran integration-tests while the VM skipped them, for the same reason. A push therefore costs about 18 billed minutes, about 110 pushes of the month's 2000 free minutes. The month's total before this run: 79.

Decision: keep the twin running while the comparison is useful and watch the counter (`wf vm exec` of the billing read); switch it off with `HOSTED_CI=off` below 500 free minutes left. Two things would make the hosted side fast: an x64 Nx cache of its own (the bucket already serves it, filled by the first run) and a runner with more cores.

## A full cold run with every deploy leg, hosted against the VM (9 Nov 2026, evening)

Hosted run 42678461649 (`[services:all] [cache:no]`, 4 shards a stage, three deploy legs at a time) against the VM CI on its warm Nx cache (run 42678319110, same commit family, one deploy leg):

| Stage | Hosted cold | VM, warm cache |
|---|---|---|
| wf build | 6 min 44 s (x64) + 5 min 06 s (arm64, for the deploy legs) | 10 s (`clis`) |
| preflight, plan | 50 s, 1 min 04 s | 1 min 03 s, 28 s |
| typecheck-lint-unit-tests | 9 min 52 s to 11 min 58 s a shard | 3 min 09 s |
| flow-tests | 14 min 03 s, 14 min 29 s, 22 min 49 s, 22 min 50 s a shard | 11 min 56 s |
| deploy legs (build, push, redeploy, serve) | 9 legs, 3 min 30 s to 11 min each, none cached (docker layers 3 of 541) | 1 leg, 1 min 39 s |
| Whole run | 1 h 01 min wall, about 190 billed minutes | 28 min 06 s |

What it says: a full cold run costs about 190 minutes, a tenth of the month's free minutes, so `[services:all] [cache:no]` is for a deliberate comparison, never a habit; a changed-only push replays the Nx cache and is far cheaper. The hosted wall time is a chain: the wf build, then the slowest flow shard (the round-robin split is uneven, 14 min against 23 min), then the image builds and the redeploys one after another. The first hosted deploys worked (8 of 9 legs shipped and served the commit); the ninth lost a request while `vm-ops-api` swapped itself. What changed after the run: 6 shards a stage, the deploy legs at once with `vm-ops-api` first and the others waiting for it only before their redeploy, and the pipeline in waves (builds, flow, typecheck, integration and cli-e2e together; wf publish, redeploy and the EAS update only after every check and test; deployment verification and the device suites last). The cap that matters is GitHub's 20 concurrent jobs on the free plan; the VM stays at one runner.

## The hosted run on the optimised shape (9 Nov 2026, night)

Hosted run 42714679278 (`c41ce74e3`, `[services:all]`, 6 shards a stage, the pipeline in waves) while the VM CI of an earlier commit (run 42714378206) was running its cold flow tests on the same VM:

| Stage | Hosted |
|---|---|
| wf build | 6 min 23 s (x64), 4 min 48 s (arm64) |
| preflight, plan | 1 min 04 s, 1 min 07 s |
| typecheck-lint-unit-tests | 6 min 51 s to 8 min 58 s a shard (build cache 7% to 20% hit) |
| flow-tests | 9 min 06 s to 18 min 12 s a shard |
| integration-tests | slowest shard 12 min 10 s |
| deploy legs | 6 of 8 green; docker layers 20 of 147 cached |
| Whole run | 34 min 09 s wall, 46 jobs green, 2 red, 4 skipped |

Two deploy legs (`vnc-auth-session`, `remote-chat-agent`) went red on the VM's containerd while pulling the image (`failed commit ... rename ... no such file or directory`, `failed to Lchown ... no such file or directory`), not on a test or on this diff; the device suites and deployment verification were skipped behind them. Eight legs pulled at once onto a VM that was also running the VM CI, so the next run says whether the pulls fail on their own or only beside a busy VM.

## Cold against warm, both flavors on the one pipeline (10 Nov 2026)

The VM CI is now `pipeline.yml` with flavor `vm` (the old job graph is gone), so the same job graph runs on both. Runs on `dev`, caches on:

| Run | Flavor | Commit | What it was | Wall | Result |
|---|---|---|---|---|---|
| 42728549672 | hosted | 710f30bdf | cold (docker layers 0 of 93 cached), every leg | 39 min 47 s | 48 jobs green; the capture deployment verification red (see below) |
| 42731636356 | hosted | 52788e147 | workflow-only push, Nx cache 97% hit | 31 min 20 s | 32 green, 4 skipped (no service changed) |
| 42737343615 | hosted | 7f039b94c | test-only push, Nx cache 100% hit | 36 min 03 s | 30 green, 6 skipped, about 164 billed minutes (rounded up per job) |
| 42737343681 | VM | 7f039b94c | test-only push, replay | 19 min 36 s | 11 green, 3 skipped |
| 42739939108 | VM | 447c12e1c | a changed `pipeline` crate (its flow tests run again, the rest replay) | 22 min 21 s | 9 green, 5 skipped |
| 42739939137 | hosted | 447c12e1c | the same push, everything replays | 7 min 56 s | 5 green, 11 skipped, about 17 billed minutes |
| 42742854241 | VM | 37371cf1c | docs and an action change, everything replays | 4 min 26 s | 8 green, 6 skipped |

What it says: with the caches on, a push that changes no service costs the VM 4 to 22 minutes of wall time (4 min 26 s when everything replays) and no billed minutes, and the hosted flavor 8 minutes and about 17 billed ones when everything replays (31 to 36 minutes and about 165 when a test stage runs again), against the cold VM run of 48 min and the cold hosted one of 34 to 61 min. The hosted floor is the two `wf` builds (about 5 and 6.5 minutes, one per architecture), which the Nx cache does not skip. The "docker layers 0 of 8" the hosted comment shows on a deploy leg is not a missed cache: `vnc-auth-session`'s image is one `FROM` of an Nx-hash-tagged image that is reused as it is, so no layer is built. Hosted wall time is not the cache's doing: run 42737343615 spent 20 min 41 s of one flow shard and 11 min 56 s of one cli-e2e shard in `install-test-deps` (the apt mirror of the hosted runner stalled, 17 minutes between two apt installs of one step; the other shards took about 2 minutes for the same step), so one stalled shard set the run's length. `install-test-deps` now writes an apt configuration with retries and a 30 s connection timeout before any apt step; the next hosted runs say whether the stalls stop.

Two reds seen and what they were: the VM flow tests failed once because the plan test ran `git diff HEAD~1 HEAD` in a depth-1 checkout (the VM flavor checks out at depth 1, hosted at full depth), fixed in 7f039b94c by giving the test an explicit `--base HEAD`; and the capture deployment verification fails from both runners (`leetcode-capture:8792` connection timed out) because the live tailnet policy has no grant for that port for the CI tags. `infra/tailscale/policy.hujson` has it (90e1317d9); applying the policy is the owner's step (#365). A cold run with every deploy leg is red on that one test until then, on either flavor.

## Stop rule

Stop dispatching and report as soon as any of these holds: a run bills more than 15 minutes; the four runs together use more than 5% of the month's free minutes; the arm runner is refused or billed above 1x; the job fails for a reason other than the toolchain (for example the crate needs the tailnet registry), in which case fix the workflow's scope, never add a secret to it. No run is repeated to improve a number.

## Commit tags

A push to `dev` is the only trigger (there is no manual one), and the commit message steers the run:

- `[services:changed]` (the default) plans what the push changed; `[services:all]` plans every test and every deploy leg; `[services:a,b]` adds the tests and the deploy legs of the named packages, crates and services to what changed.
- `[cache:yes]` (the default) replays what the Nx cache holds; `[cache:no]` runs every test again and builds every image with no layer cache and no reuse of an image of the same hash.
- `[ci:flow,unit,service]` limits the test jobs to those named.

A commit that changes no file starts the GitHub-hosted pipeline only when it carries a `[services:...]` or `[cache:...]` tag; the other empty commits are triggers of other workflows.
