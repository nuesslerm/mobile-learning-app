---
status: open
area: android-e2e
paths:
  - "e2e/console-run.lua"
opened: 2026-11-23
last_verified: 2026-11-23
links: []
---
# The console-run script has no Android unit registered

## What is broken

`e2e/console-run.lua` carries an android branch (open, chip, Run, result, sheet inside the screen) that returns early after turning the console off, but the unit is not registered in the Android list of the pipeline, so it has never run on the emulator.

## Why blocked

The emulator runs only in CI.

## What unblocks it

Register the unit in `clis/pipeline/tests/flow/pipeline.rs`, read the first hosted run, and fix or name what fails.
