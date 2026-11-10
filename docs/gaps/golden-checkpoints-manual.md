---
status: open
area: flow-tests
paths:
  - "frontend/mla-tamagui/tests/goldens/**"
  - "frontend/mla-tamagui/tests/flow/tamil-strokes.flow.test.tsx"
  - "frontend/mla-tamagui/tests/features/leetcode/ui/__tests__/ProblemEditor.flow.test.tsx"
opened: 2026-10-04
last_verified: 2026-10-20
links: []
---
# Golden screenshot checkpoints are a manual gate

## What is broken

`pnpm --filter mla-tamagui run test:goldens` renders flow-test snapshots in Chromium and diffs them
against `tests/goldens`, but nothing runs it; a visual regression in a checkpointed state is caught only
when someone does.

## Why blocked

The only automatic gate is lefthook pre-commit, which has no browser, and Chromium per commit costs minutes.

## What unblocks it

A VM-side runner that runs `test:goldens` on each push to `dev` and posts the report link, or a pre-push
hook that runs it only when files under a checkpointed route change.

## Re-checked 2026-10-20

Checked against dev at 95e494827. CI's flow-tests job still runs the plain `test:flow` vitest script (clis/pipeline/src/test_plan.rs:17, .github/workflows/deploy.yml:149), not `wf flow run --goldens`. The newest CI flow bundle, flow-f19f811d0535bdb2 (20 Oct, 79/79 pass), records env.goldens=false, so its 76 golden entries were listed but never compared. The gap holds, but its stated blocker is gone: the VM's flow-tests job already installs Chromium (deploy.yml:129) and publishes a run bundle. The fix is now to run that stage with goldens.

Out of date above:

- Why blocked says "The only automatic gate is lefthook pre-commit, which has no browser". No longer true: every push to dev runs the CI flow-tests job on the VM runner (.github/workflows/deploy.yml:104-160), which installs Chromium (deploy.yml:129) and publishes a flow run bundle. The only missing piece is that the job runs `test:flow` (plain vitest, clis/pipeline/src/test_plan.rs:17) instead of `wf flow run --goldens`.
- What unblocks it asks for "A VM-side runner that runs test:goldens on each push to dev and posts the report link". The runner and report publishing already exist (the flow-tests job plus `wf test publish`). What is left is running the flow stage with goldens (`wf flow run --goldens`, both themes, threshold 0.0001 per flow_run.rs:81) and failing CI on a mismatch.
- `test:goldens` is now `wf flow run --goldens` (frontend/mla-tamagui/package.json:39). It runs vitest once per theme (light and dark) and then `wf flow goldens`. It is not a standalone Chromium diff script. e2e-tests.yml's `--goldens` run flag (e2e-tests.yml:57) reaches only browser and Android harness runs, never flow tests.
