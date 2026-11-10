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
tags: [scheduled-tasks, clocks, claude-code, schedules-session]
---

# Scheduled tasks are clocks

A scheduled task is a **clock**: a named, thin task that fires a one-sentence tick into a persistent session which already has the repository. What to do on a tick lives in the repo (a runbook or the session's standing steps), never in the task's prompt. A standalone task that starts a fresh session has no repository (`config:no-git-repo`), cannot attach one reliably and went idle after seconds in two runs on 8 Nov 2026.

## The pattern

- **Name:** `<What> clock (<cadence>)`, for example `Contract tracker clock (daily)`, `Nightly audit clock (daily)`, `CI budget reset clock (monthly)`.
- **Target:** `persistent_session_id` of the `schedules` session (`session_01MdgrcfKcdme9NVXNuJ2fnB`) for repository work; the test-report routine fires into its own `reports` session. `create_trigger` is the only call that sets the session: `update_trigger` cannot move a task, so a wrong target means create a new task and delete the old one.
- **Prompt:** `Scheduled tick: <what is due>. The tag is [<tag>]. Do your standing steps for it (empty commit with that tag as the first line, push to dev, one-line reply).` A task whose work is analysis instead of a marker commit names its runbook: `Scheduled tick: the nightly audit review is due. Do your standing steps for it (docs/runbooks/nightly-audit.md: …; one-line reply).`
- **Tag:** when a workflow reacts to a commit message, the tick's tag is that marker (`[track t3 orca]`, `[ci-budget-reset]`, `[ts-key-watch]`, `[nx-audit]`, `[gem-sync]`), so the schedule only pushes an empty commit and the workflow does the work.
- **Cron:** `CRON_TZ=Europe/Berlin <m> <h> * * *` for a Berlin time of day, a minute that is not :00 or :30; hourly cadences use minute 0.
- **No connectors, no repository source, no model override.**

## Checklist for a new or changed task

1. Add the runbook or standing step in the repo first; the tick refers to it.
2. `create_trigger` with the name, the cron, the `schedules` session id and the tick prompt.
3. Delete the old task if one is replaced; check `list_triggers` shows the new name in the pattern.
4. Say in the runbook which task fires it (`docs/runbooks/contract-tracker.md` and `nightly-audit.md` do).
