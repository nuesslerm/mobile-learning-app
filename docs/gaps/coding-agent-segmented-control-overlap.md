---
status: open
area: coding-agent
paths:
  - "features/coding-agent-impl/src/ui/ThreadComposer.tsx"
  - "features/coding-agent-impl/src/ui/**"
opened: 2026-10-04
last_verified: 2026-10-20
links: []
---
# The Build/Plan segmented control overlaps its neighbours at 393 px

## What is broken

On the VM-captured thread screenshot (`wf vm exec --e2e`, check `coding-agent-thread`) at iPhone 15
Pro width, the selected `Build`/`Plan` segment draws over the `Plan` label and the `Model`/`Supervised`
pills. The vm-e2e-tiers `v12` vision judge splits on it (PASS once, FAIL twice on the same PNG).

## Why blocked

A coding-agent UI fix, outside the vm-e2e-tiers plan; loosening the judge would hide a real defect.

## What unblocks it

Lay the control out without overlap at 393 px in a unit with a `visual` gate on the same VM
screenshot; `v12` then passes deterministically.

## Re-checked 2026-10-20

The composer chips row and the SegmentedControl layout are unchanged since the gap opened, apart from the 18 Oct moves, and no unit has laid them out at 393 px. No CI run renders the thread screen (no PNG flow golden, no e2e test reaches it), so the overlap is neither confirmed nor refuted visually today; it stays open on the code evidence.

Out of date above:

- frontmatter paths `features/coding-agent-impl/src/ui/ThreadComposer.tsx` and `features/coding-agent-impl/src/ui/**` should be `frontend/mla-tamagui/features/coding-agent/src/ui/ThreadComposer.tsx` and `frontend/mla-tamagui/features/coding-agent/src/ui/**` (moved in 3feb94d29, 18 Oct; no -impl packages remain). Also add `frontend/libraries/tamagui-design-system/ui/segmented-control.tsx`, where the control's layout lives.
- "The vm-e2e-tiers `v12` vision judge": that plan is archived (docs/archive/README.md). `wf vm exec --e2e coding-agent-thread` was removed on 22 Oct (Metro retirement M3), and CI never ran frontend/mla-tamagui/e2e/verify-coding-agent-thread.mjs. Today's gate would be a flow screenshot golden or a browser e2e test.
- "Why blocked: ... outside the vm-e2e-tiers plan": that plan is closed, so plan scope no longer blocks the fix.
- last_verified 2026-10-04: re-verified 2026-10-20.
