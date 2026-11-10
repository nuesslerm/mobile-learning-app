---
status: open
area: vm
paths:
  - "infra/oracle-cloud/**"
  - "clis/harness-android/src/android_lab.rs"
  - ".github/workflows/device-tests.yml"
opened: 2026-10-14
last_verified: 2026-10-20
links: []
---
# The VM has almost no free memory when a device test overlaps a deploy build

## What is broken

The A1 VM (`main`: 2 OCPU, 12 GB, no swap) runs the backend, the deploy builds and the Android test device. Measured on 2026-10-14 (MemAvailable on the host):

- Two devices under test: 46 to 923 MB available.
- A deploy build alone: 281 to 503 MB available.
- One device under test uses about 1.2 to 1.4 GB.
- The buildx builder holds about 0.9 to 1.5 GB.
- One device idle with the builder running: 782 MB available.

There is no swap, so there is no slack.

On 2026-10-18 the VM had 10572 of 11927 MB in use before the device started; the device pushed it over, the dind stopped answering its health check and the watchdog recreated it, which killed the device mid-run and (before `--no-deps`) stranded the CI runner in a dead network (decision log, 18 Oct).

## Decision in place

One device. It is up only for the post-deploy test step and down after (`docker stop -t 30`, then remove). A second device was tried and dropped. The Android job starts the device only when the VM has 2000 MB available (`wf vm ops stats`), and the browser job stops its daemon before the Android job runs.

## Why it is open

A device test can still overlap a deploy build: the next push's builds can start while the test step runs. The VM is the whole Always-Free A1 pool, so it cannot grow.

## What unblocks it

1. Start the device test step only when no deploy build runs (`wf ship --status`), or stop the buildx builder while the device is up.
2. Or cap the builder's memory.
3. Or run the device somewhere other than this VM.

## Re-checked 2026-10-20

The gap still holds structurally, but its trigger has narrowed. The VM is still 12 GB with no swap. The buildx builder has no memory cap. Nothing keeps the device job and a build apart. Since 17 Oct, deploy.yml's concurrency group serializes whole runs, so the next push's builds can no longer overlap the Android job. The one build that can still overlap it is the same run's registry cache export, which wf starts after each redeploy and does not wait for (deploy_run.rs:240-258). Android units since 19 Oct never fell below 2549 MB available. CI is green through the Android job.

Out of date above:

- The path `.github/workflows/device-tests.yml` no longer exists. The device job is the `android-e2e-tests` job in deploy.yml (:330), which calls `.github/workflows/e2e-tests.yml`. The 2000 MB check is at e2e-tests.yml:145-149.
- "the next push's builds can start while the test step runs" has been false since 5c8b963e8 (17 Oct). deploy.yml:28-30 (`concurrency: group ci-<ref>, cancel-in-progress: false`) keeps the next run waiting until this run's android-e2e-tests ends. The remaining overlap is the same run's un-awaited registry cache-export buildx job (clis/deploy/src/deploy_run.rs:240-258).
- The 14 Oct figures (46 to 923 MB available with two devices, 281 to 503 MB during a deploy build) describe the old two-device setup. Across 94 CI Android units since 19 Oct, the VM never dropped below 2549 MB available (min memAvailableMinMb in the run reports).

## Re-checked 2026-10-23

The shared Metro is gone (22 Oct, stages M1 to M3), so the 14 Oct figures that counted it are one resident process lighter and nothing else in this gap changed: the VM is still 12 GB with no swap, the buildx builder has no memory cap, and the Android job still starts its device only when 2000 MB are available. The CI export steps wait for 3000 MB of `MemAvailable` before they bundle (`e2e-tests.yml`, `deploy.yml`); the figure stays until the freed memory is measured in CI.
