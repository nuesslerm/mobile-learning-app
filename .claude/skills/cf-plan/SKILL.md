---
name: cf-plan
description: Turn a requirements document into an implementation contract and a units.jsonl of 1-3 file units the git-state worker can execute and gate one by one; a cheap researcher checks the plan against the repository; then execute it on the git-state worker. Also runs the plan on the worker. Use when asked to plan or execute implementation work. Invoke as /cf-plan <slug>.
allowed-tools: Bash(wf offload ask:*), Bash(wf offload plan:*), Bash(wf offload plan --status:*), Bash(git fetch:*), Bash(git log:*), Bash(git diff --stat:*), Bash(git status:*), Bash(git add docs/plans/*), Bash(git commit:*), Read, Write
---

Input: `docs/plans/<slug>/requirements.md` (and `research.md` if present). Read them. If there is no requirements file, write a short `requirements.md` first (goal, constraints, done-when; under 120 lines) after asking the owner one question at a time.

Mode: with the `mode:non-interactive` token (or a caller that says the run is hands-off) do not pause for approval of the unit list; pin every open ambiguity in `contract.md` with the conservative choice and go on. Otherwise show the unit list and wait for a go.

1. Prior learnings (skip when `docs/plans/<slug>/learnings.md` exists and the scope did not change): `wf offload ask --agent researcher --summary=brief --caller learnings "Follow .agents/skills/cf-compound/references/learnings-researcher.md. Activity: <what the plan implements>. Decisions: <choices under consideration>. Domains: <area>."`, saved as `learnings.md`. Every solution doc it names goes into the matching unit's brief as `Read first: <paths>`. Then ONE research call for the facts the plan needs (modules, seams, test locations, conventions): `wf offload ask --agent researcher --summary=brief --caller research "<questions>"`, saved as `docs/plans/<slug>/plan-research.md`. Never explore the repo yourself beyond the plan files.
2. Write `docs/plans/<slug>/contract.md` (seams only, under 80 lines): the public shapes every unit must agree on (types, signatures, file names, error behaviour), the decisions that would otherwise be ambiguous, and the verification command for the whole change.
3. Write `docs/plans/<slug>/units.jsonl`, one JSON object per line: `{"id":"u1","title":"...","tier":"unit","files":["path"],"brief":"one imperative task + constraints, what done looks like","deps":["u0"],"gate":"<shell command from the repo root, exit 0 = pass>"}`. Optional: `"needs":"any|deployed"` (where the unit may run), `"acceptance":"<criterion>"`. Rules: 1-3 files per unit; every unit has a real gate; dependency order explicit; the unit that adds a test goes with the code it tests; nothing a human must decide inside a unit. A unit that adds or changes a screen also extends that screen's `*.flow.test.tsx` and its gate runs `pnpm --filter mla-tamagui run test:flow`.
   - **`tier`** (required: `unit`, `contract`, `flow`, `integration`, `e2e`, `visual`) names the verification rung that gates the unit. Gate per tier: `unit`/`contract`/`flow` → `cd <pkg> && pnpm exec vitest run <file>`; `integration` → the package's `test:integration` scoped to one file; `e2e` → `wf test prepare --web && wf test run <test> --platform browser --out .agents/tmp/e2e/<unit-id>` (`<test>` is the bare name of the `e2e/<name>.lua` the unit adds or extends, with no `e2e/` and no `.lua`; its header has `platform: browser`, `against: export` and `focus: true`, which `wf test run` demands and which also puts the test in every CI push, so focus only a test meant to stay. The gate's export bakes in no backend host or token (CI's browser job bakes them), so the test must reach no backend; a test that does is covered by CI after landing, not by this gate. No VM, no worktree, and `needs` stays `any`); `visual` → `wf offload worker visual` against the baseline. The final unit of a UI plan is an `e2e`/`visual` run.
4. Run `wf offload plan <slug> --check`. It parses the file without dispatching (parse errors, unknown deps, cycles, missing gate or tier, more than 3 files) and prints the dependency order. Fix every error and re-run until it ends in `ok`.
5. Commit: `git add docs/plans/<slug>/ && git commit -m "cf(<slug>): plan"`. Units land on `dev`, so land the plan there too.
6. Show the printed order and stop. Next: the "Run the plan" section below. End with `FILES_READ: <paths you read yourself>` and `ACTIONS: <one entry per step>`.

## Run the plan

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
- All landed: relay the summary line plus `git diff --stat` over the landed commits. Next step: ``wf offload ask --agent reviewer` over the plan diff <slug>`.
- End with `FILES_READ: <paths you read yourself, or none>`, `ACTIONS: <one entry per command>` and `UNITS_DISPATCHED: <n>/<units>`.
