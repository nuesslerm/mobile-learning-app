# App benchmark report

What the benchmark suite found and what changed, in priority order: 1. build speed, 2. memory, 3. runtime speed. Numbers are lab-only unless a row says device-confirmed (none are yet). Detail and method: `docs/audits/2026-10-06-app-bench-baseline.md`.

| | Start | Now | Change |
|---|---|---|---|
| Web modules | 5,682 | 3,539 | -38% |
| Web JS (raw / gzip) | 9.20 / 1.94 MB | 6.87 / 1.61 MB | -25% / -17% |
| Web cold export | 120.6 s | 116.5 s [2.1%] (quiet window, 2026-10-13); 91 to 104 s in earlier sessions | -3% here; sessions on this VM differ by about 15%, so only a same-session A/B can show less |
| iOS modules | 4,228 | 3,730 | -12% |
| iOS Hermes bytecode | 11.08 MB | 10.26 MB | -7% |
| Cold-start app heap (web lab) | 23.94 MB | 18.13 MB | -24% (goal -50% not reached) |
| Tap latency | 32.6 ms | 32.2 ms | unchanged |
| Android PSS, lab APK (slim redroid) | n/a | 216 MB (178 to 220 MB over 3 cold starts) | first baseline, lab-only |
| Android cold start to the first JS log line | n/a | 4.97 s (3.98 to 5.65 s) | first baseline, lab-only; `am start -W` timed out and is not used |
| Redroid idle memory | 792 MiB | 517 MiB (slim) | -35% |

## Why the memory goal stays at -24%

The animation stack (reanimated and gesture handler, about 330 KB of executed code) comes from the drawer shell, a product decision. On Android most of the app's 216 MB is native heap and mapped code, outside our JS, which the web lab cannot see.

## What was built

- `wf bench` suites: build, startup, memory, runtime, android, plus `compare` and an interleaved A/B for builds.
- A slim, restart-proof Android test device with a feature catalog, a lease, and a VM-run test queue that the deploy workflow drives per push.

## Device-lab gates at the final tree

Web G1 (voice edit), G2 (Tamil writing) and G3 (typing) and Android G3 pass, with recordings read back after upload: https://claude.ai/artifact/EtiP1ECtVZftaHznFFd64E. Android G2 fails (strokes were injected in about 100 ms; fixed, then still unreliable) and Android G1 was not run. G1 on the web first used the planner from the checkout, because I took the deployed planner for broken. It was not: the runbook script moved on before planning finished. On 2026-10-14 `/api/voice-plan` on the VM answered 200 in 22 s cold and 7 s warm, and browser G1 passed against the deployed agent in 22 turns. Gaps: `docs/gaps/android-tamil-gate-flaky.md`, `docs/gaps/android-editor-react-state-warning.md`, `docs/gaps/vm-voice-planner-empty-response.md`.

## Open

- Device-confirmed numbers from a phone (everything here is a lab number).
- Android G2 and G1 gates.
- A build-time claim needs a same-session A/B (`wf bench ab`); the baseline shows cold export only 3% under the start.
