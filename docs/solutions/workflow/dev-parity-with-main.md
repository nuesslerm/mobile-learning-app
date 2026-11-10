---
title: "dev must contain origin/main before any work: merge main in, never rebase a merge of it"
date: 2026-11-07
category: workflow
module: git-workflow
problem_type: workflow_issue
component: development_workflow
severity: high
applies_when:
  - "A session starts work on dev, or the open dev to main pull request was just merged"
  - "git merge-base --is-ancestor origin/main HEAD fails"
  - "A push to dev is refused with 'dev does not contain origin/main'"
symptoms:
  - "After a squash merge of dev into main, main holds one commit dev lacks and every later merge of main conflicts in about a hundred files"
  - "A push script that rebases onto origin/dev replays the squash commit and stops in a conflict-ridden detached HEAD"
related_components:
  - clis/pipeline
tags: [git, main, dev, squash-merge, rebase, parity]
---

# dev must contain origin/main before any work: merge main in, never rebase a merge of it

## Context

`dev` is the trunk and `main` receives it through the standing `dev → main` pull request, which is squash merged. A squash commit is new history on `main`, so `dev` no longer contains `main` after every merge, and any work started on that `dev` grows the gap.

## Procedure

1. `git fetch origin main dev`; create `dev` from `origin/main` if it does not exist.
2. If `git merge-base --is-ancestor origin/main dev` fails, `git merge origin/main` into `dev`.
3. Conflicts are expected because `main` is a squash of `dev`. If `main`'s tree equals the tree of some `dev` commit (loop `git rev-list origin/dev` comparing `git rev-parse <commit>^{tree}` with `git rev-parse origin/main^{tree}`), `main` has nothing unique and `git merge -s ours origin/main` is correct. Otherwise resolve by hand, and regenerate lockfiles and generated files with the repo's tooling.
4. Run the gate and push `dev` with plain `git push`; only then start work.

The pre-push gate (`wf check --pushed`) refuses a push to `dev` whose pushed commit lacks `origin/main`.

## Never rebase a range that contains a merge of main

`git rebase origin/dev` flattens merge commits and replays the squash commit from `main`: on 7 Nov 2026 this produced 95 conflicted files in a detached HEAD while the push gate ran on it. Rebase before the merge, or push the merge with `git push` alone.
