---
name: cf-lfg
description: Run the whole cf pipeline hands-off for one idea, brainstorm to documented change, every mechanical step on the cheap opencode agents, a commit after each step. Only when the user explicitly asks to build or ship autonomously or invokes /cf-lfg; otherwise use the cf-* skills one at a time. Invoke as /cf-lfg <idea> [mode:interactive].
metadata:
  invocation: user-only
  argument-hint: <idea> [mode:interactive]
allowed-tools: Bash(wf offload ask *), Bash(wf offload plan *), Bash(wf offload plan --status *), Bash(git add docs/plans/*), Bash(git commit:*), Bash(git log:*), Bash(git diff --stat:*), Bash(git status:*), Bash(git remote:*), Read, Write, Skill
---

Execute every step below IN ORDER. Do not implement anything yourself: the implementer agent writes code, the researcher agent reads the repository, you decide. Plan first, then work; never skip the plan.

The run is hands-off end to end: **you do not return to the user between steps.** The only exits are a failed GATE or the step 8 report ending in `<promise>DONE</promise>`. A sub-skill's own closing line ("Stop", "Next step: /cf-...") is written for someone invoking it directly and is SUPPRESSED here -- reaching it means continue to the next step, never hand back. **Emit no user-facing prose between steps.** Ending your turn to report is itself the hand-back, so the rule is mechanical, not a matter of intent: between step 1 and step 8 every turn you take ends in a tool call, never in prose. A step's result is recorded and carried into the step 8 report; it is not narrated when it happens. Writing "next: /cf-plan" breaks this, and so does writing "now continuing to step 2" and then stopping -- the second is the same failure wearing a promise, and it is the one that actually recurs. If a sentence addressed to the user is forming and step 8 has not been reached, delete it and make the next tool call instead.

Nothing stops to ask. Every step invokes the matching `cf-*` skill with the `mode:non-interactive` token, so unresolved choices are recorded as Decisions with the default taken. `mode:interactive` in the arguments turns the questions back on (steps 1 and 2 then wait for answers).

Every step commits what it produced (`cf(<slug>): …`; the worker lands each unit on `dev` with a `Unit: <slug>/<id>` trailer). Record `START=$(git rev-parse --short HEAD)` before step 1 so the report can list the trail.

1. **Brainstorm**: invoke `cf-brainstorm <idea> mode:non-interactive`. GATE: `docs/plans/<slug>/requirements.md` exists and its Success criteria are checkable. Otherwise stop with the reason.
2. **Plan**: invoke `cf-plan <slug> mode:non-interactive`. The plan-check researcher's output is not advisory: **apply every recommendation it makes** (or record a one-line reason for dropping it in `contract.md` under Decisions) and amend the plan commit before moving on -- a recommendation left unapplied is the defect this step exists to catch. GATE: `wf offload plan <slug> --check` printed `ok`, the plan is committed, and every recommendation is applied or explicitly dropped. Otherwise stop with the check output.
3. **Work**: for a plan touching mobile UI, share the EAS Update link from the CI run summary once a unit lands on `dev` and its run publishes the update, per the "Share the EAS Update link for mobile UI work" rule in `cf-work`; re-share the new link as visible changes land. Then arm a `Monitor` on the run before anything else, per the "Never run a dispatch you are not watching" rule in `cf-work` (PID of the `node` process found by scanning /proc, never a `pgrep -f` pattern -- it matches your own shell -- and output to a log file rather than through `tail`). Re-arm it after every timeout and after any container restart. Then invoke `cf-work <slug>` so its repair rules are loaded, which runs `wf offload plan <slug>`. On a `failed` unit apply that repair rule without asking: read only that unit's failed-ref gate output, fix the brief, the gate or the contract, delete the failed ref and re-run `wf offload plan <slug>` (landed units are never re-dispatched). At most two repair rounds; a third failure is a GATE stop: report `wf offload plan --status <slug>` and the failing unit.
4. **Simplify**: quality pass on the working-tree change -- reuse, duplication, altitude -- BEFORE review, so the reviewer reads the code that will ship rather than a draft. It does not hunt for bugs; that is step 5's job. **You do not edit code here either:** get the findings, then turn each one you accept into a unit (`id` `simpN`, the finding's files, the finding as the brief, the same gate as the unit that wrote them) and re-run `wf offload plan <slug>`. Use the `simplify` skill for the findings only, or `cf-dispatch` when the change is large enough that reading it would bloat this context. Skip only when `git diff --stat` shows no source change (a docs-only run), and say so.
5. **Review**: invoke `cf-review <slug>`. Every `blocker` or `bug` you confirm becomes one fix unit appended to `units.jsonl` (`id` `fixN`, the finding's files, the finding as the brief, the same gate as the unit it fixes); re-run `wf offload plan <slug>` once more. `risk` and `nit` items are listed in the report, not fixed. A blocker still open after the fix round is a GATE stop.
6. **Compound**: invoke `cf-compound <slug> mode:non-interactive` once per learning the run produced, naming the specific learning each time (a bare second run re-drafts the same lesson), or updating the doc it belongs in. Skip only when nothing non-trivial was learned, and say so.
7. **Ship**: `git log --oneline $START..HEAD` is the trail. Push only when `git remote` shows an origin and the session already owns a branch and PR flow; never open a new PR from here.
**Leaving the pipeline.** Repairing a plan-level blocker by hand -- a gate no unit can satisfy, an atomic
change split across units, a stale generated file -- is normal and expected. Coming back is the part that
fails: the steps after the one you left are easy to lose for hours while you hand-drive the repair. Note the
step you were on before you drop out, and re-enter at that step once the repair lands. A run is not finished
because the code is right; it is finished when Review and Compound have both happened.

8. **Report**, then `<promise>DONE</promise>`: `wf offload plan --status <slug>` totals, `wf offload ask --stats --since <run start>` spend, `git diff --stat $START..HEAD`, the review outcome (accepted / dismissed / fixed), the docs/solutions path or why it was skipped, the commit trail, and the two trailers below.

Trailers (required, last lines of the report, one line each):
`FILES_READ: <paths you read yourself, or none>` and `ACTIONS: <one short entry per step, in order>` and `UNITS_DISPATCHED: <n>/<units>`. A unit you implemented by hand counts against you: say so in ACTIONS.

Never bypass the pre-commit hook, never push per unit, never rewrite history except `--amend` on the compound commit you are correcting.
