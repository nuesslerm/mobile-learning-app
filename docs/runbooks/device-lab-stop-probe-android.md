---
title: device-lab stop probe — a timed-out Android run still ends with its report and recording
verifies: A run `wf test device` stops at its timeout (SIGTERM, then the grace period) leaves rec/report.json with verdict error and the stop reason, and a recording.
tier: agent-driven
maxAgeDays: 30
e2e: e2e/stop-probe.lua
---

# device-lab stop probe on Android

Its verdict is an error by design, so it is not focused: it does not run in CI. To run it, give it `focus: true` for that change and take it out again.

```bash
wf test device device-lab-stop-probe-android   # verdict error, "timed out after 60s", the run's rec/report.html kept
```

The script waits 900 s for an element that never renders; the runbook's 60 s timeout makes `wf test device` send SIGTERM to
the run's process group and give it 60 s to stop its recording and write its report before it is killed. A result
with `report: null` or no recording is the failure this probe exists to catch.
