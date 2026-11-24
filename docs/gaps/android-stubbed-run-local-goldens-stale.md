---
title: "The Android stubbed-run-local goldens are stale after the editor UI changes and cannot be refreshed from an agent session"
status: open
area: testing
paths:
  - e2e/stubbed-run-local.lua
  - docs/runbooks/goldens/android/two-sum-before-run.png
  - docs/runbooks/goldens/android/two-sum-stubbed-result.png
opened: 2026-11-23
last_verified: 2026-11-23
links:
  - docs/gaps/editor-cursor-android-unit-failed-in-ci.md
---

# The Android stubbed-run-local goldens are stale

On dev head fdd836ad the Android unit `stubbed-run-local` failed its screenshot compare (`two-sum-before-run` 8.9 percent over the 5 percent tolerance): the editor screen changed (no Plan/Code tab bar, no pencil or mic, the glass header layout with the settings pill). The goldens need an Android run with `-- --update-goldens` on CI; an agent session cannot dispatch that job, and `wf test goldens <runId>` could not read the published bundle (HTTP 404 on the bucket from the sandbox). The unit is unfocused (skipped, not deleted) until its goldens are refreshed by a person or a dispatched CI run: add `focus: true` back and list it in the focused Android list in clis/pipeline/tests/flow/pipeline.rs.

Since the console became the only run path the script drives Submit on the console and is browser-only (decision 110); an Android unit would need the console path proved on the emulator first, then `focus: true`, `platform: android, browser` and the list in pipeline.rs.
