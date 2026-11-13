---
title: "Scheduled tasks are clocks: a named tick into a persistent session, never a standalone prompt"
date: 2026-11-08
category: workflow
module: scheduled-tasks
problem_type: workflow_issue
component: development_workflow
severity: medium
applies_when:
  - "A recurring or one-off task is scheduled (create_trigger, a cron, a reminder)"
  - "An existing scheduled task is renamed, moved or deleted"
symptoms:
  - "A scheduled session starts without the repository and goes idle after seconds"
  - "The list of scheduled tasks mixes names and prompts of different shapes"
related_components:
  - docs/runbooks/contract-tracker.md
  - docs/runbooks/nightly-audit.md
tags: [scheduled-tasks, clocks, claude-code, schedules-session, github-schedule]
---

# Scheduled tasks are clocks

A scheduled task is a **clock**: a named, thin task that fires a one-sentence tick into a persistent session which already has the repository. What to do on a tick lives in the repo (a runbook or the session's standing steps), never in the task's prompt. A standalone task that starts a fresh session has no repository (`config:no-git-repo`), cannot attach one reliably and went idle after seconds in two runs on 8 Nov 2026.

Since the move to the public `nuesslerm/mobile-learning-app` (decisions: `docs/audits/2026-11-11-scheduled-tasks-audit.md`) a clock exists only where judgement is needed. Everything that only started a workflow is the workflow's own `schedule:` on a GitHub-hosted runner.

## The clocks

Two Claude clocks, both created by the owner or a live session against this repository, never by a code change:

| Clock | Cron | Fires into | Standing steps |
| --- | --- | --- | --- |
| `Nightly audit clock (daily)` | `CRON_TZ=Europe/Berlin 41 5 * * *` | `session_01MdgrcdJGE5MkZS29xApR9c` | `docs/runbooks/nightly-audit.md`, which ends with the contract-tracker follow-up (`docs/runbooks/contract-tracker.md`) |
| `Test report clock (every 6 h)` | `43 */6 * * *` | `session_01MdgrcfVk6KdjZ7thCpM7kp` | `docs/runbooks/test-report-routine.md` |

The two persistent sessions (`schedules`, `reports`) were created on 12 Nov 2026 with this repository as their source; ids are in the table. They need push access to `dev` under the repository ruleset (unconfirmed until the first trial tick).

## The native schedule workflows

| Workflow | Cron (UTC) | What |
| --- | --- | --- |
| `ci-audit.yml` | `23 2 * * *`, `37 3 * * 0` | the audit verdict daily; the Nx cache replay weekly |
| `track-contracts.yml` | `23 2 * * *` | the contract tracker (empty message means `[track t3 orca]`) |
| `repomix-gemini-context.yml` | `23 2 * * *`, `37 3 * * 0` | the Gem sync, cached nightly, full weekly |
| `cold-ci.yml` | `37 3 * * 0` | the weekly cold full pipeline |

Rules: GitHub runs `schedule` only from `main` and hours late, so nothing is live until `dev` is merged to `main` (at least every 50 days, because schedules are disabled after 60 days without activity on the default branch). A scheduled run never relies on a commit to start another workflow (a `GITHUB_TOKEN` push starts none). Secret-bearing jobs run only on `main` or `dev` for `schedule` or `workflow_dispatch`, with secrets mapped per job and `permissions: {}` at the top. A schedule cannot be tried before it is on `main`: the first cron-driven run of each is an acceptance step before the old clock goes.

The commit-tag markers that remain (`scheduled-triggers.yml`): `[parity]`, `[vm-state-backup]`, `[record ]`/`[replay ]`, `[hosted-minutes]`, `[hosted-tailnet]` (and `[ci-audit]`, `[gem-sync]`, `[track ]` for a manual run). A clock that needs one pushes an empty commit whose first line is the tag.

## The pattern

- **Name:** `<What> clock (<cadence>)`, for example `Nightly audit clock (daily)`.
- **Target:** `persistent_session_id` of the session that has the repository. `create_trigger` is the only call that sets the session: `update_trigger` cannot move a task, so a wrong target means create a new task and delete the old one.
- **Prompt:** `Scheduled tick: <what is due>. Do your standing steps for it (<runbook>; one-line reply).` Work that ends in a marker commit names the tag: `The tag is [<tag>]. Do your standing steps for it (empty commit with that tag as the first line, push to dev, one-line reply).`
- **Cron:** `CRON_TZ=Europe/Berlin <m> <h> * * *` for a Berlin time of day, a minute that is not :00 or :30; hourly cadences use minute 0.
- **No connectors, no repository source, no model override.**

## Checklist for a new or changed task

1. Add the runbook or standing step in the repo first; the tick refers to it.
2. `create_trigger` with the name, the cron, the session id and the tick prompt.
3. Delete the old task if one is replaced; check `list_triggers` shows the new name in the pattern.
4. Say in the runbook which task fires it (`nightly-audit.md` and `test-report-routine.md` do).
