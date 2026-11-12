---
title: nightly-audit - the nightly audit of CI on dev and the scheduled session that acts on its verdict
verifies: Each night one audit job judges the last day of CI runs on dev and prints one JSON verdict; a scheduled Claude session reads it, lands the safe fixes through the normal gate, and records what it cannot fix as a dated entry in the findings inbox issue (`#79`).
tier: runbook
---

# nightly-audit

This page is the instruction file of the scheduled Claude session "nightly audit". It replaces the former per-push budget check (`wf ci budget`, which failed or reported on every push) and the weekly Nx cache audit: both are now ONE job, `.github/workflows/ci-audit.yml`, on a GitHub-hosted runner: daily at 02:23 UTC and, with the Nx cache replay, on Sundays at 03:37 UTC (`workflow_dispatch` with `replay=true` runs the replay by hand). GitHub starts a schedule hours late and only from `main`, so nothing is live until `dev` is merged to `main`. Nothing in it gates or fails a push: `deploy.yml` has no audit step. Cost and cache health are judged over the history, because one run says little (a cold run and a replayed one differ by an hour).

The scheduled task itself is "Nightly audit clock (daily)" (cron `CRON_TZ=Europe/Berlin 41 5 * * *`, after the hosted checks): a clock like the other scheduled tasks that fires the tick "the nightly audit review is due, do your standing steps" into the persistent `session_01MdgrcdJGE5MkZS29xApR9c` (which has the repository, in `nuesslerm/mobile-learning-app`), and those steps are this page. It is created by the owner or a live session, never by a code change.

## The audit job

`wf ci audit` (schema and every finding kind: `wf ci audit --help`) reads every completed run of `deploy.yml` and `ci-hosted.yml` on `dev` of the last 24 hours (a scheduled or dispatched run counts as cold; a hosted run is judged for tasks and caches, not for time) and prints one JSON line:

- `findings[]`: `{kind, run, subject, hard, wasteSeconds, message, next}`. `hard` marks a correctness fault (the cache did not serve a result it held, refused a write, lost an entry, served a poisoned pass); the rest are cost over a threshold. `next` is the step to take.
- What it judges: CPU-seconds per run against the ledger and `cpuSecondsThreshold`, the cache layers' health (`wf ci cache-health`'s findings), a pnpm store packed by any job other than `flow-tests`, the replay ratio of every Nx stage over the window, hash drift, slower-than-history tasks, runner waits, idle gaps, I/O stalls and, for a forced cold run, jobs slower than the recent cold runs. After it the job reruns the two package stages through the cache and with `--fresh`: a kind that passes cached and fails fresh is a `cache-poisoned` finding, and the cache moves to a new generation (`wf cache purge nx`).
- Each judged run is recorded in the ledger (`HEALTH_RPT.CI_TASKS` and `CI_JOBS`) for the next night.
- Where the verdict lands: the artifact `ci-audit` (file `ci-audit.json`) and the job summary of the "CI audit" workflow run. A run with `ok: null` could not list the runs (read `reason`).
- The thresholds are in `.github/ci-audit.json` (`WF_CI_AUDIT_FILE` names another file).

## Inputs of the session

1. The newest verdict: `gh run list --workflow ci-audit.yml --limit 3`, then `gh run download <id> -n ci-audit`. **If the newest verdict is not under 24 h old, run `wf ci audit` yourself** (GitHub schedules run hours late; `--dry-run` writes no ledger rows) (it needs `GITHUB_TOKEN`, `GITHUB_REPOSITORY` and the ledger's credentials; `WF_CI_LEDGER_FILE` names a JSON lines file for a local check) and say so in the record.
2. The ledger, the last 7 days of `dev` runs (run, job and step timings; the live comment of the `dev` to `main` PR carries each run's table), `wf vm ops host --window 24h` and `7d`, and the run bundles under `verification/test-runs/`.
3. `docs/audits/2026-11-06-nx-cache-and-ci-time-audit.md` (evidence base) and `docs/audits/ci-nightly-log.md` (what earlier nights changed and expected).
4. The storage guard's line of each run (`wf: storage guard {...}` in the plan job's log, `docs/solutions/ci/cache-layers.md`): `action`, `usedBytes` against `capBytes`, `layers`, and the `quotaExceeded` count of the `wf: build cache {...}` lines. `action: skipped` on two runs in a row, or `short` twice, is a failure to fix (read `skipCode`). Never raise the cap: it is the tenancy's quota.
5. Issue `#79` (the findings inbox) and `#78` (the planned work). The owner session creates both in `nuesslerm/mobile-learning-app`; until they exist, record findings in the log only.
6. Scheduled workflows are alive: `gh run list --workflow ci-audit.yml --event schedule --limit 1` shows a last scheduled run under 7 days old. GitHub disables schedules after 60 days without activity on `main`, so also check that `dev` was merged to `main` within the last 50 days; if not, it is an owner item.

## Steps

1. **Check the earlier fixes first.** For each fix the log lists as applied in the last 7 days, compare the number it promised (minutes per run, CPU-seconds per run, replay ratio of a stage) with the measured numbers of the runs since. A fix that did not help, or made a number worse, is reverted the same night (a revert commit through the normal gate) and logged as such.
2. **Read the verdict.** Correctness (`hard`) findings first, then cost findings by `wasteSeconds` and by how many runs repeat them. One run's outlier is a note, not a finding to fix.
3. **Pick at most two fixes**, the largest avoidable cost first, only from what is safe (below). Each fix states the number it should move.
4. **Land them** through the normal gate: work on `dev` (never a session branch, never delete a branch), run the lefthook suite (never bypass), `git fetch origin dev && git rebase origin/dev`, push; confirm the remote head equals the pushed commit. Deploy is automatic. A later night checks the result (step 1).
5. **Record what you cannot fix** in issue `#79`, in the format that issue defines, as one checkbox per problem under the section that says who has to act:

   `- [ ] YYYY-MM-DD [audit] what is wrong — evidence (run id, sha or file) — why it was not fixed`

   Put it under "Open: an agent can fix this" when the next night or the tracker session can retry it, under "Open: needs the owner" when it needs a decision, a credential only a person holds, the phone, or a risk an agent should not take alone. Update an existing entry with the new evidence instead of adding a second one for the same problem; re-evaluate or move an entry older than 14 days with no progress; tick a fixed entry and add the commit sha. Use the GitHub MCP tools or `gh issue comment`/`gh issue edit` (the issue body is the list).
6. **Write the log**: append to `docs/audits/ci-nightly-log.md` the date, the numbers judged, the fixes landed (commit, expected number), the earlier fixes checked (measured number, kept or reverted) and the `#79` entries added.
7. **Adjust a threshold only with evidence.** The `cpuSecondsThreshold` (and `cpuSecondsPerExtraDeploy`, `hitRatioFloorPercent` and the others in `.github/ci-audit.json`) is a threshold the session may move, not a gate: raise it when at least five runs over at least three nights show the work is needed and stable (cite the ledger's CPU-seconds per run in the commit message and the log), lower it when the median sits well under it so a regression would show. Never move it in the same commit as the fix it would excuse, and never to make one night's number pass.

8. **Contract-tracker follow-up.** Run the Steps of `docs/runbooks/contract-tracker.md` (the tracker itself runs on its own schedule; this session reads its drift report, lands what is safe and records the rest).

## Safe fixes

Allowed: skipping a repeated step when its result does not change (a cache save that only one job needs, a restore that a hit made redundant), narrowing a cache input that the evidence shows moves for no reason, fixing a hash that differs between the sandbox and CI, timeouts that are far too long for steps that always finish, ordering or merging steps that share one install, trimming a Dockerfile COPY so an unrelated edit rebuilds nothing, metrics and logging that make a cost visible, fixing the pnpm save job guard.

Never: skipping, disabling, quarantining or loosening a test; adding a gate on a push for a cost number; changing a secret, a deploy, vm-ops-api or the runner's restart behaviour; any terraform or live VM action; deleting a branch; a change whose expected effect cannot be measured from the next runs. A fix needing more than about 150 changed lines or a new dependency is an `#79` entry, not a push.

## Guardrails

- Two fixes a night at most, and none on a night when the last night's fix has not yet been judged (fewer than three runs since).
- If the VM is busy (a CI run in progress, `wf vm ops host --window 1h` showing CPU pressure above 80 percent), do the analysis and write the log, but land nothing that needs a test run on the VM.
- The session never starts a heavy job on the VM and never prints a secret.
- The audit never fails a push; neither does anything the session lands add a step that can.

## Verifying

Run the workflow once by hand (Actions, "CI audit", Run workflow, or an empty commit to `dev` whose message carries `[ci-audit]`): the run's summary lists the findings and the artifact `ci-audit` holds the verdict. Then fire the scheduled task by hand: the log gets an entry and the push, if any, appears on `dev`. The next night's run shows the check of the first night's fix, and the first-run check in `#78` is ticked with the run and the log entry.
