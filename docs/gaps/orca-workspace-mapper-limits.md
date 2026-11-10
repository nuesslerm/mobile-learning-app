---
status: open
area: rca-parity
paths:
  - "clis/orca-serve/map-workspace/src/**"
opened: 2026-11-05
last_verified: 2026-11-05
links:
  - "docs/plans/rca-parity/plan.md"
---
# The Orca workspace mapper serves a narrower reply than the Orca host in four places

## What is broken

`orca-map-workspace` answers the Orca file and worktree rows from the rca agent API, and four replies are narrower than the pinned host's:
`files.list` and `files.readDir` stop at 5000 entries and do not honour `.gitignore`; `repo.list` ignores `afterSnapshotId` and carries no
snapshot id; `repo.hooks` reads only the `scripts:` block of the repository's `orca.yaml`; `worktree.create` ignores the parent, linked-item and
startup parameters.

## Why blocked

The rca agent API has no ignore-aware listing, no repo snapshot revision, no hooks parser and no worktree parent or startup concept; each needs
a new agent method and executor action, which no T3 or Orca row has asked for yet.

## What unblocks it

An orcad oracle run (VM and CI emulator) that shows a phone flow depending on one of the four, then the agent method and executor action for it.
