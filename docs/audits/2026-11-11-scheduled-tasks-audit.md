# Scheduled tasks audit, 2026-11-11

Why: the repository moved to the public `nuesslerm/mobile-learning-app`. Eight Claude scheduled tasks still fired into one persistent session
of the old private repository, pushing empty marker commits to its `dev`. This audit decides what each one is for, where it should run, and
how often. Every clock was read against the code (read-only), then the plan was attacked from three sides (GitHub platform limits, secret
exposure on a public repository, coverage). The defects they found are folded into the decisions below.

## What each clock was, and what happens to it

| Clock (old) | What it did | Decision |
| --- | --- | --- |
| Nightly audit clock (daily) | Claude review of the CI audit verdict, up to two safe fixes, the log, owner items | **Stays a Claude clock**, recreated against the new repo, runs the contract-tracker follow-up too |
| Contract tracker clock (daily) | marker `[track t3 orca]` -> `track-contracts.yml` | **Becomes a native `schedule:` run on a hosted runner**; drift reaches the nightly review through the run history |
| Gem sync clock (nightly) | marker `[gem-sync]` -> `repomix-gemini-context.yml` | **Native `schedule:` run on a hosted runner**; the cached form nightly, the full form weekly |
| Cold CI clock (weekly) | marker `[cold-ci]`: a cold full pipeline | **Native weekly `schedule:` run on a hosted runner** (free minutes on a public repository) |
| Nx cache audit clock (weekly) | marker `[nx-audit]` | **Deleted**: its work is the replay half of `ci-audit.yml`, which moves to the weekly run |
| CI budget reset clock (monthly) | marker `[ci-budget-reset]` | **Deleted**: the budget exists for private-repository minutes; a public repository has none |
| Tailscale key watch clock (weekly) | marker `[ts-key-watch]` | **Deleted**: its workflows were removed on purpose, the OAuth client is a vm-scope secret and must never reach public CI |
| test-report routine (every 6 h) | Claude session that builds one report page from run bundles | **Stays a Claude clock**, own `reports` session, recreated against the new repo |
| Wiki weekly lint | the owner's personal vault on their own machine | **Out of scope**, untouched |

"Run on GitHub-hosted containers" applies to the work a workflow does. A Claude session cannot run on a GitHub container, so the two clocks that
need judgement remain Claude clocks and everything that only started a workflow is now the workflow's own `schedule:`.

## What can run together

- **Daily, hosted** (one cron, `23 2 * * *` UTC): the CI audit verdict, the contract-tracker plan, the cached gem sync. All are HTTP or cassette work
  with no VM state; they share setup (tailnet join as `tag:ci-gh-hosted`, one `wf` build).
- **Weekly, hosted** (`37 3 * * 0` UTC): the cold full pipeline, the audit replay (Nx stages cached against `--fresh`), the full gem sync. These are
  the heavy, slow-changing checks; weekly is enough because Nx inputs and base images change over days.
- **Claude, daily** (`CRON_TZ=Europe/Berlin 41 5 * * *`): the nightly review, after the hosted checks, and it checks that the newest verdict is under
  24 h old and otherwise runs `wf ci audit` itself, because GitHub schedules run hours late.
- **Claude, every 6 h** (`43 */6 * * *`, `reports` session): the test report; bursts of run bundles follow pushes, so 6 h is enough.

## Constraints the plan must respect (from the adversarial checks)

1. GitHub runs `schedule:` workflows only from the default branch (`main`) and starts them hours late (5.7 to 7 h observed). Workflow edits land on
   `dev`, and nothing is live until `dev` is merged to `main`. The Berlin-04 hour guard in `ci-audit.yml` is removed; it skipped every run.
2. GitHub disables scheduled workflows after 60 days without activity on the default branch. The nightly review checks that the last scheduled run
   is under 7 days old, and `dev` is merged to `main` at least every 50 days.
3. Commits pushed with `GITHUB_TOKEN` start no workflows, so a scheduled workflow never relies on a commit to start another.
4. A secret-bearing job runs only on `main` or `dev` and only for `schedule` or `workflow_dispatch`; secrets are mapped explicitly per job, never
   `secrets: inherit`; permissions are `{}` at the top and granted per job. The tracker's capture legs, which run provisioned third-party code, never
   hold `BWS_CI_DEPLOY_ACCESS_TOKEN` before the record step.
5. `pipeline.yml` has no `fresh` or `services` inputs, so the weekly cold run needs them added (or dispatches `ci-hosted.yml` with a message).
6. A scheduled run cannot be tried before the workflow exists on `main`; the first cron-driven run of each is an acceptance step before the old
   clock is deleted.
7. Keep a native check that the tailnet auth key has more than 14 days left (`wf vm net auth-key status`) as an owner-side or VM-side timer, since
   the removed workflow was the only automated watch.
8. The new Claude sessions need push access to `dev` under the repository ruleset; unconfirmed until the first trial tick.

## Order of work

1. Disable the old clocks (done 2026-11-11; none fires into the old repository any more).
2. Finish the hosted and self-hosted parity checks, then discard the `expo-hello-world-app-2` checkout.
3. Land the workflow and runbook edits on `dev`, merge `dev` to `main` (the owner's decision), create the `schedules` and `reports` sessions and the two
   Claude clocks, then trial each: `fire_trigger` for the Claude clocks, `workflow_dispatch` for the hosted ones.
4. Delete each old clock only after its replacement has run green once.
