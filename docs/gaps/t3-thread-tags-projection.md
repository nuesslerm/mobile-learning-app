---
status: open
area: rca-parity
paths:
  - "clis/t3-serve/core/src/usecases/forward.rs"
  - "clis/t3-serve/core/src/domain/decider.rs"
opened: 2026-11-20
last_verified: 2026-11-20
links:
  - "docs/plans/rca-parity/plan.md"
---
# A tag set on a thread is kept but not shown in the thread projection

## What is broken

`thread.tag.set` is accepted and stored in the agent API state (`thread.tag:<thread>:<tag>`), but `orchestration.getThreadProjection` does not list it: the recorded server answers
`thread.tags: ["a"]` after the command, this server leaves `tags` out. The replay row `t3-scn-command-thread.tag.set` carries the difference in `known-gaps.json`.

## Why blocked

The projection is built from the engine's thread row, which the tag command does not touch (the tag lives in state, with no list read). No recorded projection of a thread with no
tag shows whether `tags` is `[]` or absent, so the empty case is not known either.

## What unblocks it

Keep the tags on the thread row (an event that sets `tags`) and answer them from there; record a projection after an untag to learn the empty shape.
