---
title: "Parallelism telemetry: in-runner and cross-runner"
date: 2026-11-20
category: ci
module: ci-pipeline
problem_type: workflow_issue
component: development_workflow
severity: medium
applies_when:
  - "Choosing `--parallel N` for a test stage, or how many jobs and shards a workflow runs"
  - "Deciding whether to merge jobs (each runner pays a fixed cost) or split them (a run is as slow as its slowest job)"
  - "The account's 20 concurrent hosted jobs are the limit and runs queue"
tags: [ci, telemetry, parallelism, runners, ci-database, betterstack]
---

## The two questions

Hosted runners are 4 vCPU / 16 GB and the account runs at most 20 jobs at once, across every caller workflow (`ci-hosted-backend-core.yml`, `-mobile-core`, `-cli-core`, `-cli-build`, `-backend-build`, `-mobile-build`; all call `pipeline.yml`). Two things decide how fast a push is green and what it costs:

- **In-runner parallelism**: is `wf test run ... --parallel 2` right for 4 cores and 16 GB? Too low leaves the machine idle, too high thrashes it (CPU pressure, swap, memory peak).
- **Cross-runner parallelism**: every extra job pays a fixed cost (checkout, toolchain, cache restore; measured: report 52 s, preflight 58 s, plan 104 s, wf-build 131 s) and takes one of the 20 slots; fewer, fuller jobs leave headroom for other runs, more, smaller jobs shorten the run until the cap makes them queue.

Both are answered from the sealed CI database (`wf ci db query "<sql>"`, SQLite) and from Better Stack gauges. Nothing here is a gate; every verb is best effort.

## What is measured, and by whom

| Source | Writes | Where it runs |
|---|---|---|
| `.github/actions/hosted-setup/sampler.sh` | `stats-<job>.jsonl` every 5 s | beside every hosted job (artifact `stats-<job>`) |
| `wf ci resources [--send]` | `CI_JOB_RESOURCES`, `CI_TASK_RESOURCES` | report job, after the `stats-*` and `ci-db-*` artifacts are downloaded and before `wf ci db merge` |
| `wf ci timings [--send]` | `CI_JOBS` (+ `QUEUED_S`, `RUNNER`), `CI_STEPS`, `CI_RUN_PROFILE` | report job (already runs) |
| `wf ci concurrency [--phase start\|end]` | `CI_CONCURRENCY` | any job; the first job and the report job of each workflow give a start and an end reading |
| `wf test run --stage` (Nx ledger) | `NX_TASKS` with `STARTED_MS`, `ENDED_MS`, `CPU_S` | each test job's own database |
| `wf test run --stage` (Nx output) | `CI_TESTS`, gauge `ci.test.duration_s` | each test job's own database |

Wiring is in the workflows, not in the verbs: the verbs take job names from the data (the artifact name, the Actions API job name), never from a list, so a renamed or merged job needs no code change. The rows go to a job-local database (`WF_CI_DB`, else `$RUNNER_TEMP/wf-ci-db/rows.sqlite`); the job's `hosted-stats upload` step ships it and the report job's `wf ci db merge` folds it into the sealed one.

### The sampler (5 s, backward compatible)

Each JSONL line keeps the old keys (`t, cpu, memUsedMb, memTotalMb, load1`; `wf ci comment` still reads only those) and adds `iowaitPct`, `swapUsedMb`, `procsRunning`, `readBytes` / `writeBytes` (block I/O since the last sample, from `/proc/vmstat`), and `psiCpu`, `psiMem`, `psiIo` (kernel pressure `avg10` of `/proc/pressure/*`, `null` when the kernel has none). The first sample alone carries `cores` and `runner` (`RUNNER_NAME`, the join key to `CI_JOBS.RUNNER`).

## Tables

All timestamps are epoch seconds unless the column says `_MS`. The keys make a repeated write one row.

**`CI_JOB_RESOURCES`**, one row per job (`RUN_ID, JOB`): `WORKFLOW, RUNNER, CORES, SAMPLES, CPU_AVG, CPU_P95` (nearest rank), `CPU_MAX, IOWAIT_AVG, MEM_AVG_MB, MEM_PEAK_MB, MEM_TOTAL_MB, SWAP_PEAK_MB, LOAD1_MAX, PSI_CPU_MAX, PSI_MEM_MAX, PSI_IO_MAX, READ_MB, WRITE_MB, DURATION_S` (first to last sample plus one interval), `IDLE_S` (seconds with CPU under 25%), `SATURATED_S` (seconds with CPU over 90% or CPU pressure over 20). `JOB` is the artifact name (`core-tests`, `flow-tests-3`); join to `CI_JOBS` on `RUN_ID` and `RUNNER`, not on the job name.

**`CI_TASK_RESOURCES`**, one row per Nx task that ran (`RUN_ID, JOB, TASK, STARTED`): `ENDED, SECONDS, CPU_S, CORES_USED` (`CPU_S / SECONDS`), `CONCURRENT` (tasks of the job overlapping it, itself included), `CPU_AVG` and `MEM_PEAK_MB` (the job's samples over the task's interval; null when none overlaps). Nx reports CPU per `run-many`, never per task, so `CPU_S` is the stage's `wf: nx cost` `cpuSeconds` split by each ran task's milliseconds; read `CPU_AVG` and `CONCURRENT` beside it. Replayed tasks have no row.

**`CI_TESTS`**, one row per slow or failed test of a weighted stage (`RUN_ID, JOB, STAGE, UNIT, SUITE, TEST`): `MS, OUTCOME` (`ok` or `failed`), `BASIS`. `UNIT` is the same unit as in `CI_UNITS` (`crate-wf`, `crate-t3-serve/replay-...`), `SUITE` the libtest binary (`flow`, `lib`, `integration`) or the vitest file, `TEST` the libtest path or the vitest `suite > test`. Stable libtest prints no test time, so nothing runs twice and no flag changes (a changed command would move the task hash): `wf test run --stage` reads the Nx output it already forwards to stderr and stamps each result line as it arrives. `BASIS` says how far to trust `MS`:

| `BASIS` | Meaning |
|---|---|
| `serial` | the target runs `--test-threads=1` (`cargo-integration*`, `cargo-e2e*`, `cargo-verify-deployed`, `replay-*`): the time since the previous result is the test's own |
| `gap` | tests run in parallel: the time since the previous result of the same project (or since its binary started). It is the quiet stretch before the result, not the test's own time: a fast test that finishes after a long one's start shows that stretch, and the slow one that finishes right after shows almost none, so the test it names is unreliable (not a lower bound). A `gap` row is never a gauge, and an unknown start (a result before any `Running` line) reads 0 |
| `reported` | the runner printed the time (`<1.250s>` from `--report-time`, or a vitest `12ms`), exact |

Passing tests under 250 ms are not rows (a crate has thousands); every failed test is, until a later call of the stage passes it (that row replaces it). A task Nx replayed from its cache has no rows (its output is old), the output of a failed test's `---- name stdout ----` section is not read, and the same title twice in one file keeps the failed or longer one. Nx's `project: ` prefix is the only handle on a line, so two targets of one crate running at once share a lane: their rows can carry the wrong suite or time. A test of 10 s or more is also the gauge `ci.test.duration_s{stage, unit::test}`, a row of `CI_METRICS`. The rows are job-local and the report job's `wf ci db merge` folds them in, like `CI_UNITS`. A vitest line carries its time only when the reporter prints it (the default prints one line per file, `verbose` one per test); `--reporter=json` is not wired, since the commands are the packages' scripts and a changed script moves the Nx hash.

What the slowest tests of a crate are:
```sql
SELECT UNIT, SUITE, TEST, MS, BASIS FROM CI_TESTS WHERE UNIT = 'crate-deploy' ORDER BY MS DESC LIMIT 10;
```
and which test got slower than its median (a trend per test: `SELECT TEST, RUN_ID, MS FROM CI_TESTS WHERE TEST = ? ORDER BY RUN_ID`).

**`CI_STEPS`**, one row per step of every finished job (`RUN_ID, JOB, STEP_NO`): `STEP, STARTED, COMPLETED, DURATION_S, CONCLUSION`. `JOB` is the API job name, as in `CI_JOBS`.

**`CI_JOBS`** gained `QUEUED_S` (job `started_at` less `created_at`; null when it never started) and `RUNNER` (`runner_name`). `QUEUED` stays (it also counts the wait of a job cancelled while pending).

**`CI_RUN_PROFILE`**, one row per run (`RUN_ID`): `WORKFLOW, WALL_S` (run start to the last job's end), `RUNNER_S` (sum of job durations), `PEAK_JOBS` (most jobs running at once inside the run), `AVG_JOBS` (`RUNNER_S / WALL_S`), `QUEUE_S_TOTAL, QUEUE_S_MAX, JOBS, CRITICAL_PATH_JOB, CRITICAL_PATH_S, PARALLEL_EFFICIENCY` (`RUNNER_S / (WALL_S * PEAK_JOBS)`). Skipped jobs are left out. The Actions API does not say which job needs which, so the critical path is the chain from the last job to finish back through whichever job ended last at or before each one's start (a 2 s hand-over tolerance), joined with ` > `; `CRITICAL_PATH_S` is its first start to the last end. The report job itself is not finished when `wf ci timings` runs, so a profile covers the jobs before it.

**`CI_CONCURRENCY`**, one row per workflow and one for `all` per reading (`RUN_ID, PHASE, TS, WORKFLOW`): `BUSY` (jobs in progress), `QUEUED`, `RUNS_ACTIVE`, `CAP` (20; `WF_CI_RUNNER_CAP` overrides), `HEADROOM` (the account's: `CAP` less all busy jobs, repeated on each row). The reading is the repository's in-progress and queued runs from the Actions API; another repository on the same account is not seen.

## In the PR comment

Each of the six hosted workflows keeps one comment on the pull request (`CI (<label>)`), edited in place by `wf ci live` and finished by `wf ci comment`. The comment is built in the report job **before** `wf ci db merge`, so apart from the Trend section below it does not query the sealed database: it recomputes the same numbers in process (`clis/pipeline/src/ci_telemetry.rs`) from the raw inputs the `wf ci timings|resources|concurrency` verbs read. The report job therefore downloads `ci-db-*` first and gives "Report the run" `WF_CI_LEDGERS`, `WF_CI_CORE_SHARD_KEYS`, `WF_CI_SHARD_KEYS` and `WF_CI_CLI_E2E_SHARD_KEYS`.

| Line in the comment | Source |
|---|---|
| Headline: wall, runner time, peak and average jobs at once, parallel efficiency, jobs that ran; critical path in the verdict | `ci_timings::profile` over the jobs that ran (the `CI_RUN_PROFILE` numbers) |
| Fixed cost table: `setup-job`, `install-test-deps` steps, the `wf-build` and `plan` jobs | `ci_timings::step_rows` (`CI_STEPS`) |
| Queue wait, and a long wait with its concurrency group (workflow, shard index and key from `WF_CI_CORE_SHARD_KEYS`, `WF_CI_SHARD_KEYS`, `WF_CI_CLI_E2E_SHARD_KEYS`) | job `created_at` to `started_at` (`CI_JOBS.QUEUED_S`) |
| Runner pool: repo busy / queued / headroom at plan and at report | the plan job's `CI_CONCURRENCY` start row; the end reading is taken in process |
| Runner table: vCPU and memory as sampled, CPU avg/p95/peak, iowait, memory, swap, pressure, seconds idle and saturated | the sampler's `stats-<job>.jsonl` (`CI_JOB_RESOURCES`); cores come from the sampler, never a constant |
| In-runner parallelism: tasks at once per job and the cores they used | `ci-db-<job>/rows.sqlite` `NX_TASKS` with the samples (`CI_TASK_RESOURCES`) |

**Trend** (`clis/pipeline/src/ci_trend.rs`) is the one section that does read the sealed database, and the history it reads does not hold this run (the merge comes after the comment). For a hosted run (`CI_RUN_PROFILE.WORKFLOW` contains `+`) it shows:

| Part | Source |
|---|---|
| A one-row table: this run's wall, critical path and queue against the median of the last 20 successful runs of the same workflow, each with an arrow (up slower, down faster, right within 10%) | this run's profile; `CI_RUN_PROFILE` `WALL_S`, `CRITICAL_PATH_S`, `QUEUE_S_TOTAL` |
| `Longest:` the job of this run's critical path that ran longest, and its longest step (setup and post steps left out) | the run's jobs and their steps |
| One mermaid `xychart-beta` line chart per workflow of the wall time in minutes, oldest first, for the workflows with at least two runs; this run's workflow first, at most 5 charts, so the six hosted workflows never make six | `CI_RUN_PROFILE` |

A successful run is one with no job whose `CONCLUSION` is `failure`, `cancelled` or null (skipped is fine), the same rule as the `wf ci db query` analyses. The tables keep no branch: only the pipeline of `dev` merges rows, so the history is dev's. The database is opened as `wf ci db query` opens it (`CI_DB_KEY`, from the environment or the vault, then the sealed file on `refs/wf/state`; `WF_CI_SEALED_FILE` names a sealed file instead, for tests and offline replay). When it cannot be read or holds no history for the run's workflow, the section is left out and nothing else in the comment changes; a run that is still going, or not a hosted one, gets no section either. The charts and table together are about 3 KB, far under the 60,000 character cap.

Only jobs that ran are rows; the jobs of the shared `pipeline.yml` that the caller's `stages`/`area` left out are one count under the table. The e2e units are one collapsed block with the pass and fail counts in its summary (open only when a unit failed, failing rows first). Cache health is one row per kind with the job count; `WF_CI_PUSH_GATE=off` (set for the whole pipeline) means a hosted push seeds nothing, so no `replayed-nothing` problem is raised. Stale comments of older heads and of retired labels are removed by `wf ci prune` (run after every live write); see `docs/reference/wf-cli.md`.

## Better Stack gauges

Same record as `CI_METRICS`; labels are `scope` and `name` (see [betterstack-query-recipes](../workflow/betterstack-query-recipes.md)).

| Gauge | scope | name | From |
|---|---|---|---|
| `ci.runners.busy`, `ci.runners.queued`, `ci.runners.headroom` | workflow (`all` for the account) | phase (`start`, `end`) | `wf ci concurrency` |
| `ci.job.cpu_avg`, `cpu_p95`, `cpu_max`, `iowait_avg`, `mem_peak_mb`, `swap_peak_mb`, `psi_cpu_max`, `psi_mem_max`, `psi_io_max`, `idle_s`, `saturated_s` | job | workflow | `wf ci resources --send` |
| `ci.run.runner_s`, `peak_jobs`, `avg_jobs`, `queue_s_total`, `queue_s_max`, `critical_path_s`, `parallel_efficiency` | run id | workflow | `wf ci timings --send` |

## Recipes (`wf ci db query "<sql>"`)

### Fixed overhead share per job

How much of each job is setup that every runner pays. The step patterns are what the hosted jobs run before and after their own work; edit them when the setup steps change.

```sql
SELECT j.JOB, COUNT(*) AS runs, ROUND(AVG(j.SECS)) AS job_s, ROUND(AVG(o.overhead_s)) AS overhead_s,
       ROUND(100.0 * SUM(o.overhead_s) / SUM(j.SECS), 1) AS overhead_pct
FROM CI_JOBS j
JOIN (SELECT RUN_ID, JOB, SUM(DURATION_S) AS overhead_s FROM CI_STEPS
      WHERE STEP IN ('Set up job', 'Complete job') OR STEP LIKE 'Run actions/%' OR STEP LIKE 'Run ./.github/actions/%' OR STEP LIKE 'Post %'
      GROUP BY RUN_ID, JOB) o ON o.RUN_ID = j.RUN_ID AND o.JOB = j.JOB
WHERE j.CONCLUSION = 'success' AND j.STARTED > 0
GROUP BY j.JOB ORDER BY overhead_pct DESC;
```

A job whose `overhead_pct` is high and whose `job_s` is short is a candidate to fold into a neighbour (it costs a runner slot and a fixed cost for little work). The slowest steps by name:

```sql
SELECT JOB, STEP, COUNT(*) AS n, ROUND(AVG(DURATION_S)) AS avg_s, MAX(DURATION_S) AS max_s FROM CI_STEPS
WHERE DURATION_S >= 5 GROUP BY JOB, STEP ORDER BY AVG(DURATION_S) * COUNT(*) DESC LIMIT 30;
```

### In-runner saturation: is `--parallel 4` right?

```sql
SELECT JOB, COUNT(*) AS runs, MAX(CORES) AS cores, ROUND(AVG(CPU_AVG)) AS cpu_avg, ROUND(AVG(CPU_P95)) AS cpu_p95, ROUND(MAX(CPU_MAX)) AS cpu_max,
       ROUND(MAX(PSI_CPU_MAX), 1) AS psi_cpu, ROUND(MAX(PSI_MEM_MAX), 1) AS psi_mem, ROUND(MAX(PSI_IO_MAX), 1) AS psi_io,
       ROUND(100.0 * MAX(MEM_PEAK_MB) / MAX(MEM_TOTAL_MB)) AS mem_peak_pct, MAX(SWAP_PEAK_MB) AS swap_mb,
       ROUND(100.0 * SUM(IDLE_S) / SUM(DURATION_S)) AS idle_pct, ROUND(100.0 * SUM(SATURATED_S) / SUM(DURATION_S)) AS saturated_pct
FROM CI_JOB_RESOURCES GROUP BY JOB ORDER BY saturated_pct DESC;
```

Reading it (starting points, judge with the wall time of the job beside it):

| What the row says | Meaning | Move |
|---|---|---|
| `idle_pct` high, `cpu_p95` under 60, `mem_peak_pct` under 60, `psi_cpu` near 0 | the runner waits (serial phases, I/O) | raise `--parallel`, or fold a neighbour job into the idle stretch |
| `saturated_pct` high, `psi_cpu` over 20, memory fine | CPU is the limit; more parallelism only queues | keep or lower `--parallel`; split the job across runners instead |
| `psi_mem` over 10, `swap_mb` over 0, or `mem_peak_pct` over 85 | memory pressure; processes are being reclaimed | lower `--parallel`, or split the heavy tasks across runners |
| `psi_io` over 20 with `write_mb` large | disk bound (cache restore, `cargo` target, `node_modules`) | change the cache layout, not the parallelism |

Per task (which tasks use how many cores, and how many ran together):

```sql
SELECT TASK, COUNT(*) AS runs, ROUND(AVG(SECONDS), 1) AS avg_s, ROUND(AVG(CORES_USED), 2) AS cores_used, ROUND(AVG(CONCURRENT), 1) AS together,
       ROUND(AVG(CPU_AVG)) AS cpu_avg_seen, ROUND(MAX(MEM_PEAK_MB)) AS mem_peak_mb
FROM CI_TASK_RESOURCES GROUP BY TASK ORDER BY SUM(SECONDS) DESC LIMIT 20;
```

A task that uses under 1 core while `together` is 1 is serial work another task could share the runner with.

The runner behind a job name (the join between the two naming schemes):

```sql
SELECT j.JOB, r.JOB AS stats_job, r.CPU_P95, r.MEM_PEAK_MB FROM CI_JOBS j JOIN CI_JOB_RESOURCES r ON r.RUN_ID = j.RUN_ID AND r.RUNNER = j.RUNNER
WHERE j.RUN_ID = '<run id>' ORDER BY j.JOB;
```

### Cross-runner concurrency timeline vs the 20 cap

Concurrent jobs per minute across all runs of the last day, from the jobs' own intervals (`CI_JOBS.STARTED` to `STARTED + SECS`; a job counts in every minute it overlaps). It sees the jobs of every run whose report job ran `wf ci timings`.

```sql
WITH RECURSIVE m(minute) AS (
  SELECT (CAST(strftime('%s', 'now', '-1 day') AS INTEGER) / 60) * 60
  UNION ALL SELECT minute + 60 FROM m WHERE minute < CAST(strftime('%s', 'now') AS INTEGER)
)
SELECT datetime(minute, 'unixepoch') AS at, COUNT(j.JOB) AS busy, 20 - COUNT(j.JOB) AS headroom
FROM m LEFT JOIN CI_JOBS j ON j.STARTED > 0 AND j.STARTED < m.minute + 60 AND j.STARTED + j.SECS > m.minute
GROUP BY minute ORDER BY minute;
```

How often the cap bound (wrap the same CTE):

```sql
WITH RECURSIVE m(minute) AS (
  SELECT (CAST(strftime('%s', 'now', '-7 day') AS INTEGER) / 60) * 60
  UNION ALL SELECT minute + 60 FROM m WHERE minute < CAST(strftime('%s', 'now') AS INTEGER)
), per_minute AS (
  SELECT minute, COUNT(j.JOB) AS busy FROM m LEFT JOIN CI_JOBS j ON j.STARTED > 0 AND j.STARTED < m.minute + 60 AND j.STARTED + j.SECS > m.minute GROUP BY minute
)
SELECT MAX(busy) AS peak, SUM(busy >= 20) AS minutes_at_cap, SUM(busy >= 16) AS minutes_near_cap, SUM(busy > 0) AS minutes_active FROM per_minute;
```

The live readings (what the API said when a run started and ended, including queued jobs the interval table cannot see):

```sql
SELECT datetime(TS, 'unixepoch') AS at, RUN_ID, PHASE, BUSY, QUEUED, RUNS_ACTIVE, HEADROOM FROM CI_CONCURRENCY WHERE WORKFLOW = 'all' ORDER BY TS DESC LIMIT 50;
```

A `QUEUED` above 0 with `HEADROOM` 0 is the cap binding. A `QUEUED` above 0 with headroom left is GitHub's own start delay, not the cap.

### Queue wait by hour

```sql
SELECT strftime('%Y-%m-%d %H:00', STARTED, 'unixepoch') AS hour, COUNT(*) AS jobs, ROUND(AVG(QUEUED_S), 1) AS avg_wait_s, MAX(QUEUED_S) AS max_wait_s, SUM(QUEUED_S > 30) AS over_30s
FROM CI_JOBS WHERE STARTED > 0 AND QUEUED_S IS NOT NULL GROUP BY hour ORDER BY hour DESC LIMIT 48;
```

### Runner-seconds per workflow

```sql
SELECT WORKFLOW, COUNT(*) AS runs, ROUND(SUM(RUNNER_S) / 3600.0, 2) AS runner_hours, ROUND(AVG(RUNNER_S)) AS avg_runner_s, ROUND(AVG(WALL_S)) AS avg_wall_s,
       ROUND(AVG(PEAK_JOBS), 1) AS avg_peak_jobs, ROUND(AVG(PARALLEL_EFFICIENCY), 2) AS efficiency, ROUND(AVG(QUEUE_S_TOTAL)) AS queue_s
FROM CI_RUN_PROFILE GROUP BY WORKFLOW ORDER BY runner_hours DESC;
```

Where a workflow's runner-seconds go, by job:

```sql
SELECT p.WORKFLOW, j.KIND, COUNT(*) AS jobs, ROUND(SUM(j.SECS) / 3600.0, 2) AS runner_hours, ROUND(AVG(j.SECS)) AS avg_s
FROM CI_JOBS j JOIN CI_RUN_PROFILE p ON p.RUN_ID = j.RUN_ID WHERE j.STARTED > 0 GROUP BY p.WORKFLOW, j.KIND ORDER BY runner_hours DESC;
```

The runs furthest from ideal parallelism, and what holds them back:

```sql
SELECT RUN_ID, WORKFLOW, WALL_S, RUNNER_S, PEAK_JOBS, ROUND(PARALLEL_EFFICIENCY, 2) AS efficiency, CRITICAL_PATH_S, CRITICAL_PATH_JOB FROM CI_RUN_PROFILE ORDER BY PARALLEL_EFFICIENCY LIMIT 20;
```

Low efficiency with a `CRITICAL_PATH_S` close to `WALL_S` is a serial chain (shorten or split its slowest link); low efficiency with a long chain of short jobs is fixed overhead (fold them).

## Limits worth knowing

- Samples are 5 s apart: a task shorter than that sees one sample, so `CPU_AVG` is a coarse number for it.
- `CI_TASK_RESOURCES` exists for jobs whose database reached the report job (`ci-db-<job>` artifact) and whose Nx ledger recorded intervals; a job that ran before the intervals existed has `STARTED_MS` 0 and no row.
- `CI_TESTS` is written when the stage's Nx run ends, so a shard cancelled while hung leaves no test rows; the run's last printed `test ... ok` line in the job log is then the nearest evidence of which test it was in. A process a test leaves running can hold Nx's output open: the capture stops waiting 2 s after Nx exits.
- The 20-job cap is an account limit; `CI_CONCURRENCY` and the interval timeline see this repository only.
- A job cancelled while pending ran 0 s: it is in `CI_JOBS` with `STARTED` 0, and in no timeline.
