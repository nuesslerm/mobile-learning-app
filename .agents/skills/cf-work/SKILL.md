---
name: cf-work
description: Execute a plan's units.jsonl on the git-state worker (each unit implemented by rca on the cheap ladder, gated, landed on dev), then report the outcome. Use after /cf-plan, or with any units.jsonl. Claude never implements a unit itself here. Invoke as /cf-work <slug>.
allowed-tools: Bash(wf offload plan:*), Bash(wf offload plan --status:*), Bash(git fetch:*), Bash(git log:*), Bash(git diff --stat:*), Bash(git status:*)
---

Validate first: `wf offload plan <slug> --check` must end in `ok`; fix `units.jsonl` until it does. Then run one command and wait for it (minutes to hours: each unit is a real rca run plus its gate):

```bash
wf offload plan <slug> [--max N] [--site sandbox|vm] > .agents/tmp/work-<slug>.log 2>&1
```

The worker leases each ready unit through a git ref (`refs/leases/<slug>/<id>`), implements it in a worktree via `rca run`, runs its gate, and lands it on `dev` as one commit with the trailer `Unit: <slug>/<id>`. A unit whose gate keeps failing gets `refs/failed/<slug>/<id>` holding the gate output. It ends with `landed N [...], failed N [...], released N [...]`; stderr carries one JSON line per event.

`wf offload plan --status <slug>` shows every unit as `done | leased | failed | blocked | runnable` from those refs at any time, from any checkout; a sandbox worker and a VM worker (`--site vm`) can share one plan.

## Share the EAS Update link for mobile UI work

A push to `dev` that changed the app publishes an EAS Update (the `eas-update` job, after deploy and deployment verification); its link is the `### EAS Update` line of the CI run summary (a push that left the app unchanged publishes none: `wf ship eas --latest` prints the last one's link). A plan that touches mobile UI shares that link once a unit has landed on `dev` and its run published the update, and again whenever a later unit lands a visible change. The update is immutable (no Fast Refresh); `wf ship eas --initial-route <route>` publishes a branch by hand, baking only the `EXPO_PUBLIC_*` variables of your environment (good for UI, not for the backends). Until it lands, verify with flow tests and a local `expo export --platform web` driven by `wf browser`.

## Never run a worker you are not watching

Arm a `Monitor` on the worker before doing anything else. Use the PID of the `node` process whose cmdline contains `work <slug>` after `wf.js` (the worker now runs in-process) (scan `/proc`; a `pgrep -f` pattern matches the monitor's own shell). Emit new `Unit: <slug>/` commits on `origin/dev` and the log's outcome lines; on `kill -0` failure, run `wf offload plan --status <slug>`. Re-arm after every timeout or container restart.

## After the run

- Re-running is resuming: landed units are read from `dev` trailers and never re-dispatched.
- Steer: edit `units.jsonl` (split a unit, sharpen a brief or gate), land it, re-run. Cancel: SIGTERM the worker; it releases its leases.
- A `failed` unit: read only its gate output (`git fetch origin 'refs/failed/<slug>/*:refs/failed/<slug>/*' && git log -1 --format=%B refs/failed/<slug>/<id>`). Classify it: the unit's spec is wrong → fix `units.jsonl`; the worker or rca has a bug → fix that code (normal, not an escalation); a genuine human ambiguity → stop and ask. Then delete the failed ref (`git push origin :refs/failed/<slug>/<id>`) and re-run. Never implement the unit yourself.
- All landed: relay the summary line plus `git diff --stat` over the landed commits. Next step: `/cf-review <slug>`.
- End with `FILES_READ: <paths you read yourself, or none>`, `ACTIONS: <one entry per command>` and `UNITS_DISPATCHED: <n>/<units>`.
