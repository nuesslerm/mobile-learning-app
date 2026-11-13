---
status: open
area: gemini
paths:
  - "clis/knowledge/src/knowledge_gemini.rs"
  - ".github/workflows/repomix-gemini-context.yml"
  - "infra/oracle-cloud/agent-shell/Dockerfile"
opened: 2026-10-04
last_verified: 2026-10-27
links: []
---
# The Gemini bundle sync's second run and the Gem's answers are unverified

## What is broken

The first real run worked (2026-10-27, `repomix-gemini-context.yml` on the then self-hosted VM runner (it runs hosted now), run 40600211256, 8 minutes): the draft of `ARCHITECTURE.md` came from the rca service on its first attempt with the inputs inline in the prompt (225 s in a sandbox run), the full repomix pack followed (15.6 M characters, 1,257 files), and the bundle was copied over the Drive file `repomix-context.md` in the folder `Gemini Gem context` (read back: 15,603,239 bytes, `ARCHITECTURE.md` first, then the pack). What has not been seen:

- a second run over the same file (the nightly at 03:17 UTC): the file id must stay and the size and modified time must change.
- the Gem's answers from that one file (the three questions of `gemini-gem-large-doc-grounding`), which need the Gem in the Gemini app.
- `ARCHITECTURE.md` read end to end against its checklist (layering rules, every `features/*`, backend service and `clis/*` crate): its headings and first rules look right, the rest was not read.
- the nine Google Docs of the earlier multi-doc pipeline are orphaned in Drive and can be deleted by hand.

## What unblocks it

Read the file id and `modifiedTime` of `repomix-context.md` after the next nightly, attach the file to the Gem and ask the three grounding questions; record the result in `gemini-gem-large-doc-grounding`. The login behind the job is the vault mirror `GCLOUD_DRIVE_CREDENTIALS_DB_B64`, which `wf secret login gcloud --clear` empties until a new login sets it again.

## 2026-10-30 check

- No scheduled run happened at 03:17 UTC: GitHub fires `schedule` only from the default branch, and the workflow lives on `dev` (every run so far is a `push`). The nightly needs the workflow on `main`, which the `dev` → `main` merge brings.
- Push run 17 (`4e480e8`) failed at the draft: the rca service skipped every rung for cold pools, and the client printed "All 0 models were exhausted". The message now names the skipped pools. The cause is the shared cooldowns after the parity e2e load; the draft honors cold pools because it pins no model.

## 2026-10-31 change

- `dev` → `main` (#349, 56f0d54) put the workflow with its `schedule` on `main`, so a scheduled run can now start; none has been seen yet.
- The manual dispatch is gone. The sync is triggered by an empty commit to `dev` whose message carries `[gem-sync]` (`git commit --allow-empty -m "[gem-sync]" && git push origin dev`); the CI pipeline starts an empty commit only with a `[services:...]` or `[cache:...]` tag, so that commit starts no CI run. Not yet seen live: a `[gem-sync]` push that runs the Gem job and no CI run.
- The schedule is a test phase (every ten minutes, reusing the cached `ARCHITECTURE.md`), to see runs queue in the `gemini-context` concurrency group and overwrite the same Drive file; it goes back to nightly afterwards.
- Every upload now reads the destination back and writes `uploaded file id=… size=… modified=…` to the job summary (one file of that name, the bundle's size, modified within 15 minutes), so "the same file id, a newer modified time" is read from the run, not remembered. Tests: 3 unit (verdict and time parsing), the flow tests, and `tests/integration.rs` (the workflow's own environment and sequence over a local folder; Drive itself is proven only by the live runs).
- Still open: the live runs above, the Gem's answers, `ARCHITECTURE.md` read end to end, the nine orphaned Google Docs.

## 2026-10-31 live check of the empty-commit trigger

- Empty commit 286e746 (`[gem-sync]`): no CI run started, and the Gem run's job was skipped, because the push payload of an empty commit has `added`, `modified` and `removed` as `null`, not `[]`, so the job-level expression over them was false. A `gate` job now counts the files with `jq` (`null` has length 0) and logs `{"added":null,"modified":null,"removed":null}`.
- Empty commit 123535c (`[gem-sync]`): again no CI run; `gate` and the sync both green (run 41220136392; it queued behind the CI run of the commit before it on the one runner). The sync's flow (5) and integration (5) tests passed, then it drafted `ARCHITECTURE.md`, packed 1,279 files (15,873,496 chars) and uploaded; the read-back wrote `uploaded file id=1eraRTRw… size=15881840 modified=2026-10-31T16:56:06.654Z`. First Drive upload since 29 Oct, so the rca draft and the vault login both worked today.
- Still not seen: a scheduled run (needs the ten-minute cron on `main`), the same file id on a second run, queued runs in the concurrency group, the Gem's answers.

## 2026-10-31 evening: GitHub's schedule is hours late here

- #351 (the ten-minute cron on `main`) was merged at 17:50 UTC; 63 minutes later no scheduled run had started. This private repo's scheduled workflows start hours after their cron (weekly Tailscale watch `0 6 * * 1` fired at 14:20, 12:01 and 12:12 UTC on the last three Mondays; the Nx audit `17 3 * * 1` at 09:03), so a ten-minute cadence cannot be tested through `schedule` here.
- The clock is now a Claude scheduled task that pushes an empty `[gem-sync]` (or `[gem-sync cached]`, which reuses the cached draft) commit to `dev`; the workflow's cron is back to nightly as the late backup. The ten-minute test runs as a chain of such pushes from the session; the nightly one is a scheduled task.

## 2026-10-31 late: every cron removed

- The owner removed all GitHub cron triggers (GitHub starts them hours late in this private repo; the ten-minute test never produced a run): the Gem sync, the weekly cold CI, the weekly Nx cache audit, the weekly Tailscale auth-key watch and the monthly CI-budget reset. The Gem sync now runs only for an empty `[gem-sync]` or `[gem-sync cached]` commit to `dev`. The Claude scheduled task that pushed that commit nightly was removed too.
- What stops happening by itself: a nightly refresh of the Gem's file, the cold-run baseline for `wf ci budget` (`cold-slower`), the cache audit, the 90-day Tailscale key warning, and the monthly reset of `CI_BUDGET_LOW`. Each workflow keeps its `workflow_dispatch`.

## 2026-10-31 night: the crons became Claude scheduled tasks

- The first fire of the nightly clock (a test, 18:57 UTC) pushed nothing: the fired session had no repository and looked for `add_repo` with ToolSearch, but it is a normal tool of the session, not a deferred one. The task prompts now say to call `mcp__claude-code-remote__add_repo` directly.
- The gate that decides whether a tagged commit is empty read the push payload's `added/modified/removed`, which are null for every commit, so a commit whose message merely mentioned the tag ran the sync (e6c74d7 did, at 18:55 UTC: its upload, 15,881,840 bytes, is the Drive file modified 19:57 local). The shared `tag-gate.yml` now asks the commits API instead.
- Each former cron is a tagged empty commit to `dev` that a scheduled task pushes: Gem sync nightly `[gem-sync]`, Nx cache audit weekly `[nx-audit]`, Tailscale key watch weekly `[ts-key-watch]`, CI budget reset monthly `[ci-budget-reset]`.

## 2026-11-11: the Gem sync is a native schedule again

On the public repository the sync runs from its own `schedule` on a hosted runner (cached nightly, full on Sundays); the Claude clock that pushed `[gem-sync]` is retired. Decisions: `docs/audits/2026-11-11-scheduled-tasks-audit.md`.
