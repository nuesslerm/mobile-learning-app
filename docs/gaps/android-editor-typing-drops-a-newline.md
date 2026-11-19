---
status: open
area: android-e2e
paths:
  - "e2e/editor-typing.lua"
opened: 2026-11-19
last_verified: 2026-11-19
links: []
---
# The Android editor-typing unit sometimes loses a typed newline, and the run then fails at the Run step

## What is broken

`editor-typing-problem-two-sum-part-2` on the hosted Android job (run 44618453035, 70b3b92a) typed `return [seen[target - n], i]` and `seen[n] = i` as one line: the Enter and the indent between them never reached the editor (the screenshot `wait-timeout-*.png` of that unit shows `i]seen[n] = i`, and the app answered `CompileError: invalid syntax (<string>, line 6)`). Most runs pass this unit; the script's own comment names the same unit for an earlier miss (18 Oct, two-sum part 2). The dropped key is an `adb input` press, not the app.

## Why blocked

Not blocked. `read_back` already notices the merged line (a line has to read back whole, so both lines are missing), but `lines_match` is asserted only after the Run step, which times out first on `ProblemEditorRunOutcomeTotals` and hides the cause, and only a missing suffix of the last line is ever retyped.

## What unblocks it

Assert the read-back before the Run step and retype the whole part once when a line is missing. Proving it needs an emulator, which the sandbox does not have (the emulator runs only in CI), so the change is checked by the Android job of a hosted run.
