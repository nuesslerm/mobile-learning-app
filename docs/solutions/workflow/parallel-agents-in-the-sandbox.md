---
title: "Parallel agents in the sandbox: worktrees, one cargo lock, disk floor"
date: 2026-11-08
category: workflow
module: agent-instructions
problem_type: workflow_issue
component: development_workflow
severity: medium
applies_when:
  - "Several independent issue items are worked at once"
  - "The sandbox disk is low"
tags: [worktree, flock, parallel]
---

# Parallel agents in the sandbox

Moved verbatim from the always-on `CLAUDE.md` section "Parallel agents in the sandbox" (8 Nov 2026), where a short rule now points here.

Work as many independent issue items at once as the sandbox holds (2 cores, little disk). Each item is one agent in its own git worktree (`isolation: worktree`), which commits there and never pushes, never touches the main checkout, never runs `git checkout`, `reset`, `stash` or `clean` in a shared tree, never deletes a branch, never edits the issue bodies (it reports an evidence line); the lead cherry-picks each commit onto `dev` through the push gate. Every `cargo`, `pnpm` test and `wf check|test` command runs as `flock /tmp/cargo.lock <command>` so builds take turns while the agents think and edit in parallel, with one `CARGO_TARGET_DIR` per workspace (`/tmp/shared-cargo-target-orca` for `clis`, a subdirectory for each other workspace) so dependencies compile once. After a verification delete the test executables it left (`find $CARGO_TARGET_DIR/debug/deps -maxdepth 1 -type f -perm -u+x ! -name '*.so' -delete`); at under 1.2 GB free (`df -h /`) an agent stops and reports what is done. Items that edit the same crate or file run one after the other, not side by side.
