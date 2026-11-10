---
status: open
area: vm-infra
paths:
  - "infra/oracle-cloud/agent-shell/**"
  - "clis/deploy/src/build_image.rs"
  - "clis/wf-vmops/src/vm_exec.rs"
  - "clis/deploy/src/agent_deps.rs"
  - "clis/deploy/src/deploy_run.rs"
opened: 2026-09-15
last_verified: 2026-10-20
links:
  - gaps/vm-has-no-second-door.md
  - archive/solutions/integration-issues/tailnet-hostname-is-not-the-name-that-resolves.md
---
# The VM stops answering on the tailnet during a heavy `wf ship`

## What is broken

During `wf ship remote-chat-agent`, after the whisper.cpp compile and push, every tailnet
HTTP call (`vm-ops-api-main:8080/health`, `remote-chat-agent-prod`, `leetcode-backend-prod`) timed out
at once while `tailscale status` showed the peer active with tx/rx climbing and OCI reported RUNNING.

## Why blocked

Suspected memory pressure (at the time of the incident a 4 GB Node heap for `pnpm deploy`, since replaced
by the `cargo build -p rca-service` stage, plus the whisper.cpp compile beside every running app on the A1),
unproven. Host memory is readable through vm-ops-api (`wf vm ops stats`), which is what stops, and through
reporting-service (`reporting-service:8791/api/metrics/series`), which samples `/proc/meminfo` every 60 s into
SQLite on a named volume, so the samples taken up to a hang survive a reset. Both answer only on the tailnet
that stops, so neither can be read live during the hang. The only out-of-band reading is the agent-shell
heartbeat's `memUsedMb` in BetterStack (`wf vm logs --service agent-shell`), every 15 min, and nothing
records PSI.

## What unblocks it

Expose memory pressure (PSI), and an out-of-band host-memory reading finer than the 15-min agent-shell
heartbeat (BetterStack `memUsedMb`), in the watchdog or by pushing reporting-service's 60 s host samples off
the VM. vm-ops-api `/stats` already gives host memory and load while it is up, and it reads
`/proc/pressure` only to supervise its own self-redeploy build; no route exposes PSI and the watchdog reads
no memory at all. Cap the build's memory, and give the release a recovery path of its own: `wf vm recover`
(wait up to `--max-wait` for vm-ops-api, OCI SOFTRESET then RESET while it stays unreachable, then a health
sweep that redeploys unhealthy apps with recreate=all) is the only current lever, at the cost of the build.
Note the resolving tailnet name can flip between `-2` and bare.

## Re-checked 2026-10-20

The hang recurred twice. On 19 Oct (CI run 39307044623 at dcab33aae) a cold Nx flow stage running four cargo and vitest builds at once took the whole VM off the tailnet for about 27 minutes. On 18 Oct (at 0150f765c) memory pressure from the device made agent-shell-dind stop answering (decision log lines 2420 and 2454). CI now has preventive levers: `--parallel 2` for the flow and typecheck stages (deploy.yml:149,207), a check for 2000 MB available before the device starts (e2e-tests.yml:148), and builds that run at cpu-shares 128 (build_image.rs:206) and wait while a device run is up. Still, no route or watchdog exposes PSI, and the new WF_VM_SAMPLE_SECS sampler polls /stats in-band over the tailnet during Android runs only. Nothing caps a build's memory, so the gap holds.

Out of date above:

- "`wf vm recover` ... is the only current lever": it is still the only recovery lever, but since 18-19 Oct CI also has preventive ones. The flow and typecheck stages run `--parallel 2` (.github/workflows/deploy.yml:149,207). The Android job refuses to start the device below 2000 MB available (.github/workflows/e2e-tests.yml:148). Image builds yield CPU through `--cpu-shares 128` (clis/deploy/src/build_image.rs:206) and wait while a device run is up (9d44b94ff). None of these caps memory.
- "nothing records PSI": almost true. vm-ops-api logged PSI attributes in `self_build.waiting_for_headroom` during its own self-build, which is gone (it no longer builds). wf's worker reads /proc/pressure/memory for admission (clis/offload/src/worker_run.rs:557) and stores nothing.
- The gap names only the 15 Sep `wf ship remote-chat-agent` incident. It should add the 18 Oct (memory pressure from the device, 10572/11927 MB used) and 19 Oct (VM off the tailnet 19:04-19:31 during a cold four-wide Nx flow stage) recurrences from docs/audits/2026-09-02-token-diet-decision-log.md:2420 and :2454.
