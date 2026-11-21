---
status: open
area: browser-e2e
paths:
  - "e2e/local-run.lua"
opened: 2026-11-21
last_verified: 2026-11-21
links: []
---
# local-run.lua taps Run Local without asserting the mock keyboard is down

## What is broken

A tap on Run Local while the phone frame's mock keyboard is up lands on the key under the button and types a character into the code. `e2e/run-error-line.lua` asserts the keyboard is down before it runs; `e2e/local-run.lua` `lower_keyboard` waits only 3 s for the rail and does not check, so on a slow runner a letter could be typed before a run.

## Why blocked

Not blocked. It has not failed on a hosted run.

## What unblocks it

Make `lower_keyboard` wait on `not device:get_device_state('keyboard').visible` (the failure bound only) and share it with `run-error-line.lua`.
