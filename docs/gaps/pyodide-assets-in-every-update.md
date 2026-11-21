---
status: open
area: leetcode
paths:
  - "frontend/mla-tamagui/features/leetcode/data/pyodide/assets.ts"
  - "frontend/mla-tamagui/metro.config.js"
opened: 2026-11-20
last_verified: 2026-11-20
links: []
---
# The bundled Pyodide adds 12.9 MiB to every EAS Update and every web deploy

## What is broken

The on-device Python runner bundles the Pyodide runtime (decision 3 of issue #86): `pyodide.asm.wasm` 9.15 MiB, `python_stdlib.zip` 2.43 MiB, the loader 1.19 MiB and the lock file 0.11 MiB, 12.9 MiB in all (about 6.1 MiB deflated). They are plain Metro assets, so they enter the manifest of every update and every web export, not only the route that uses them (the spike route, later the runner). expo-updates downloads every manifest asset when it loads an update and fails the whole load when one download fails, so a flaky phone connection can now fail an update of the app's other work too. Before the spike an update's assets were about 200 KB.

## Why blocked

Not blocked: bundling is the owner's decision, and the cost is the price of running code with no network and no VM.

## What unblocks it

Measure a cold update load on a phone connection once the runner ships (B5). If it hurts, the options are a smaller runtime (MicroPython, noted in `docs/plans/leetcode-foundation/spec.md`), or assets that are not in the manifest and are fetched once into the cache directory on first use (which gives up the airplane-mode first run).
