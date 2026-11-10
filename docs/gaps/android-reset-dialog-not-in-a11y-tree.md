---
status: open
area: leetcode
paths:
  - "frontend/mla-tamagui/features/leetcode/ui/ResetCodeDialog.tsx"
  - "frontend/mla-tamagui/features/leetcode/routes/[id]/reset-code.tsx"
opened: 2026-10-12
last_verified: 2026-10-20
links: []
---
# The "Reset to starter code?" dialog is missing from Android's accessibility tree

## What is broken

On the Android emulator the dialog draws, but `uiautomator dump` shows none of its nodes: `ResetCodeDialog-Confirm` is "not found" while the buttons behind it are listed. Earlier Android runs found it. A screen reader would not reach the buttons either. The Lua runbooks tap the Reset button at its drawn position (`xy:400,578` at 540x960) as a stopgap.

## Why blocked

Not blocked; the cause (the modal route's window, or a view hiding its descendants from accessibility) is not yet known.

## What unblocks it

Find why the dialog's nodes are hidden from accessibility on Android, fix it, add a flow assertion on the accessibility props, then drop the `xy:` fallback from `editor-typing.lua` and `voice-edit.lua`.

## Re-checked 2026-10-20

I read the per-verb timelines of the unseeded Android CI runs from the last two weeks. The dialog's Confirm button was missed by id in 6 runs between 17 and 18 Oct, the last on 18 Oct 18:11. All 37 unseeded runs since then (14 commits, including today's green CI) found it in the accessibility tree within 3 s and tapped it by id. The symptom is gone, most likely since the reused-app fix (acee60739/e1194cab1), but no dialog code changed. The xy fallback is still in e2e/editor-typing.lua:217-227 and e2e/voice-edit.lua:162-172, where it would hide a recurrence. Closing the gap should remove that fallback.

A closure was proposed and a skeptical review kept it open: The gap is still open. The checker shows that the symptom stopped appearing in CI, but nothing shows a fix.

Out of date above:

- "The Lua runbooks tap the Reset button at its drawn position (`xy:400,578` at 540x960)": they are now e2e tests (e2e/editor-typing.lua:217-227, e2e/voice-edit.lua:162-172). The point is scaled with across(400)/from_middle(578) to 356,607 on the 480x1040 device, after a pixel check that the dialog is drawn, and only when `id:ResetCodeDialog-Confirm` does not appear within 3 s.
- "`uiautomator dump` shows none of its nodes": Android finds now go through the UI server (android_u2). In CI since 18 Oct 18:11 the node is found every time (37/37 unseeded runs).
- "the cause ... is not yet known": the misses stopped when a reused app began leaving the problem screen for Settings before reopening it (acee60739, e1194cab1, editor-typing.lua:195-200). That is a likely cause, not a proven one.
