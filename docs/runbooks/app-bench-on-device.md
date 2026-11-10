---
title: App benchmark, on-device confirmation of the lab memory numbers
verifies: That the lab-only heap numbers `wf bench memory` reports move the same way on a real phone, measured with the platform's own tools
tier: manual
packages: clis/app-bench, frontend/mla-tamagui
maxAgeDays: 180
---

# App benchmark: on-device confirmation

`wf bench memory` measures the V8 JS heap of the lab web export (Chromium at iPhone 15 Pro size). That number is lab-only: it does not include native view memory, bitmaps, the Hermes heap, or the split GCs a phone runs. Run this recipe before claiming a memory win as device-confirmed. It needs a desktop with a phone or simulator attached, so it is a manual runbook and is not part of the `wf bench` gate.

## What to compare

Same build, same scenario, two readings: the lab heap from `wf bench memory --suite memory` (cold start, then each route in `clis/app-bench/scenarios/memory.json`), and the phone's own number for the same steps. The two will not match in absolute terms. What has to match is direction and rough ratio: a change that cuts the lab cold-start heap by 20% should not leave the phone's resident memory flat.

## Setup

1. Export the production bundle the phone will run, from the commit under test: `pnpm --filter mla-tamagui exec expo export --platform ios` (and `android`). Check the Hermes bytecode size `wf bench build` already records for the iOS export.
2. Install it the way a release is installed (EAS Update on a development client, or a release build). A debug build keeps the dev menu and a larger JS heap and will mislead.
3. Quit every other app; let the phone idle for a minute so background work settles.

## Android: `adb shell dumpsys meminfo`

1. Cold start the app: `adb shell am force-stop <package>`, then launch it from the launcher.
2. Wait 10 s on the first screen. Read `adb shell dumpsys meminfo <package>`; note `TOTAL PSS`, `Java Heap`, `Native Heap`, `Graphics` and `Views`.
3. Open each route of the memory scenario once and come back, reading the same numbers after each. Note `Views` and `Activities`: a `Views` count that only grows is a retained screen.
4. Repeat the open and back loop 30 times on one route. `TOTAL PSS` should settle; a staircase is a leak.

## iOS: Instruments

1. Xcode, Instruments, the Allocations template (and Leaks), attach to the app by name.
2. Mark generations: one after cold start, one after each route. The growth between marks for a route you opened and left should be near zero after a few seconds.
3. Read `Persistent Bytes` after cold start for the comparison with the lab cold-start heap.

## What to record

Append to `docs/audits/<date>-app-bench-baseline.md`, under `Device-confirmed`: the commit, the device and OS, the build type, the numbers above next to the lab number for the same step, and whether the direction agrees. Keep the lab-only label on every number the phone did not produce.

## Not covered

Hermes heap snapshots from the Chrome DevTools attach, and React Native's own view-count tooling. Add them here when someone runs them.
