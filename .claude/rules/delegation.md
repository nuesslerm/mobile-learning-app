# Delegation to cheap models

Check every user request against this table before your first tool call.

| Task shape | Do |
| --- | --- |
| 1 file, ≤10 lines, location already known; a single Read/Grep; a question answerable from what is already in context | **Direct.** A dispatch takes 40–400 s and costs more than the read. |
| 1 file, non-trivial logic or unknown location; 2–3 files, one goal; procedural research with a concrete question; anything whose output would be large in your context (>1.5 KB) | **Delegate:** `delegate` MCP tool, or `wf offload ask --summary=brief "…"`, or `/cheap <task>`. |
| >3 files or multi-step logic; a batch of the same change across files | **Decompose**, one delegate call per unit (or `wf offload ask --batch <glob> -t template.md`); `git diff --stat` between units. |
| Architecture or design decisions, planning, code review, security-sensitive changes, live debugging where each result decides the next step | **Stays on you.** |

## Which model

Every dispatch runs on the rca ladder, not on a hand-picked model. Plans run on the git-state worker via `wf offload plan <slug>` / `wf offload plan --status <slug>` (sandbox or `--site vm`); to pivot, edit `units.jsonl`, land it and re-run:

- Ladder file: `libraries/rca-core/plugins/ladder/data/model-ladders.json` (rungs by `Shape = \`implementer:${Tier}\` | 'judge:text' | 'judge:vision' | 'document' | 'runbook'`).
- On any non-fatal failure (`wrote-nothing`, `bad-output`, `crash`, `timeout`, `cap`) the wrapper rotates to the next rung; a second identical failure escalates one strength level; `fatal` stops the loop.
- Pool cooldowns (the shared `HEALTH_RPT.POOL_COOLDOWNS` table, read and written by the rca service) drop a capped pool from the ladder for `learnedResetMs` (initial 5 h); `observeOk` shrinks, a cap after recovery grows it by 25 %. Every ask runs on the rca service (`wf offload ask --files …` sends the files it reads and gets what it changed back); only `--runbook` still drives a local opencode.
- Haiku is automatic; Sonnet escalates from Haiku automatically. `claude-opus-5` and `claude-fable-5-1` (`automatic: false`) are never tried unless the unit pins them explicitly.
- `wf offload ask --stats --json` is the evidence source — `okRate`, `p50Ms`, `cost` per `(shape, tier, model, pool)` group. Pin only when the stats justify it.

How to delegate well:

- One task per prompt. Name the exact files. Give `Stack`, `TASK` (one imperative), `CONSTRAINTS`, and a `VERIFY: grep …` line instead of asking it to re-read files.
- Relay the summary it returns verbatim (with `--summary=brief`: its final message plus the tokens/cost line). Do not verify it and do not re-read the changed files unless the message hedges, contradicts the task, or reports an error. The full transcript is at the `transcript` path if you need it.
- `WROTE_NOTHING` means it described the change instead of making it: fix the prompt ("make the edit now"), retry. Max 3 attempts, then do it yourself.
- Never delegate through the `Agent` tool: a subagent spawn costs ~100K cache-write tokens before it does anything. A `delegate` call or a Bash dispatch rides your warm cache.
- `wf offload ask --stats` shows whether delegation is paying off in this repo.

## UI units and the dev landing rule

A unit that touches `frontend/**` UI gates on the flow tests, not a grep: `pnpm --filter mla-tamagui run test:flow` plus the affected `*.flow.test.tsx` (Given/When/Then, rendered-DOM assertions, harness doubles, no `vi.mock` of project modules). Land on the `dev` trunk: `git fetch origin dev && git rebase origin/dev`, run the lefthook suite, push. Before any trunk-tied test tier, verify the remote head equals the pushed commit -- an empty `git log HEAD..origin/dev` after `git fetch origin dev` (or `git status -sb` showing no "behind"); see `docs/solutions/workflow/stale-checkout-mistaken-for-missing-code.md`.
