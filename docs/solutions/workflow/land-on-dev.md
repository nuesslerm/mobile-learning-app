---
title: "Land on the dev trunk: the standing dev to main pull request, rebase and push rules"
date: 2026-11-08
category: workflow
module: git-workflow
problem_type: workflow_issue
component: development_workflow
severity: high
applies_when:
  - "Starting work, landing a change or the dev to main pull request was merged"
tags: [dev, main, push-gate]
---

# Land on dev

Moved verbatim from the always-on `CLAUDE.md` section "Land on dev" (8 Nov 2026), where a short rule now points here.

**9 Nov 2026, owner's call:** the lefthook `pre-push` job (`wf check --pushed`, the sandbox run of the CI stages that preseeds the shared Nx cache) is removed while the hosted CI takes over more of the test work: a push is gated by the pre-commit hook only, and CI runs the stages cold on its own runners. Restore it by reverting the commit that removed it.

When the open `dev` → `main` PR is merged or closed, open a new `dev` → `main` PR at once, titled exactly `dev → main` with the body exactly ``This is `dev` `` (the PR is the trunk's standing target, not a feature branch: never put a changelog in it). Work autonomously: ask the owner as little as possible. Before any work, bring `dev` to parity with `main`: create `dev` from `origin/main` if missing, then merge `origin/main` into `dev` when it has commits `dev` lacks; the pre-push gate refuses a push to `dev` without them (procedure and the rebase hazard: `docs/solutions/workflow/dev-parity-with-main.md`). Land on the `dev` trunk: `git fetch origin dev && git rebase origin/dev` (never over a merge of `main`: push that with plain `git push`), run the lefthook suite, push. Deploy is automatic on push to `dev` -- CI ships only the services whose package graph changed; never call `/redeploy` by hand. Before any trunk-tied test tier, confirm the remote head equals the pushed commit (`docs/solutions/workflow/stale-checkout-mistaken-for-missing-code.md`). Work on `dev`, not on a session branch. Never delete a branch, local or remote (spike, plan, dev or stale): stale branches are kept and ignored.

## Small batches, landed as they pass (22 Nov 2026)

A restart of the sandbox (several on 22 Nov, probably memory pressure from builds in `/dev/shm`) wiped build outputs and any worktree under `/dev/shm`, and the agents that held a day of uncommitted work could not commit it quickly because the gate refuses code that does not build. The rule that follows:

- One unit per commit: the files of one behavior or fix, its package tests, then the gate. Do not batch a feature's pieces into one final commit.
- Commit and push before starting the next unit; the push does not wait for CI (see "Never wait on CI").
- A delegated or workflow agent is told the same in its brief, and told to commit and push after each finished step.
- Work trees outside the main checkout live on disk (`/home/user/...`), not in `/dev/shm`.
- Build only the packages you changed; cargo is capped at 2 jobs in `~/.cargo/config.toml`.

