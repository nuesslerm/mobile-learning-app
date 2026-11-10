---
status: blocked
area: vm-infra
paths:
  - "infra/oracle-cloud/vm-ops-api/**"
  - "infra/oracle-cloud/watchdog/**"
  - "backend/watchdog/**"
  - "backend/vm-ops-api/**"
  - "clis/deploy/src/vm_drill.rs"
  - "infra/oracle-cloud/dummy-test-infra.tf"
opened: 2026-08-15
last_verified: 2026-10-23
links:
  - solutions/infra/vm-code-deployment-flow.md
  - solutions/integration-issues/a-tls-only-egress-silently-swallows-every-non-tls-protocol.md
---
# vm-ops-api is the only way into the VM; there is no break-glass path

## What is broken

When vm-ops-api stops answering (its tailnet device stays registered; `curl: (97) Can't complete
SOCKS5 connection`), every deploy, `exec-script` build and rsync into agent-shell fails with it, and the
recovery for a dead vm-ops-api is vm-ops-api.

## Why blocked

- OCI Run Command never registers in this tenancy: "Compute Instance Run Command" is absent from the
  agent's reported plugin list even on a freshly recreated instance, though the catalog marks it
  enabled-by-default for Ubuntu 24.04, so the Run-Command `oci service` commands were removed (2026-10).
- SSH from a sandbox is impossible: egress is HTTPS-only (port 22 times out) and the VM host is not a
  tailnet peer (`SOCKSv5 error: General SOCKS server failure`). The console connection tunnels over SSH too.

## What unblocks it

Build a second door that does not depend on vm-ops-api: the SSH-over-Tailscale-Funnel break-glass
spec, the watchdog container recreating vm-ops-api on failed health, or OCI support enabling Run
Command. Confirm with vm-ops-api stopped on purpose and `wf vm ops health` recovering to `ok`.

The watchdog is the door (owner decision, 21 Oct 2026, `docs/plans/vm-ops-api-rs/plan.md` section 5, "Second door"). It is
built and tested in code; it closes this gap only when both drills have run live, each recovered by the watchdog and not by a
person:

```
wf vm ops drill stop   --json | jq -e '.recovered == true and .by == "watchdog"'
wf vm ops drill freeze --json | jq -e '.recovered == true and .by == "watchdog"'
```

Each needs the owner's approval for the run (it takes the control plane down), and a deployed service and watchdog at the commit
that carries the code below.

## Re-checked 2026-10-23

What exists now, none of it run against the VM yet (the owner's go for a live run is the open step):

- **A stopped control plane is started again** (D2a). The watchdog recreates a container of compose project `vm-ops-api` that has been
  exited for more than two minutes while its service is still declared by the project's compose file
  (`docker compose config --services`), with reason `exited` and the same ten-minute cooldown as its other rules
  (`backend/watchdog/core/src/domain/decide.rs`). Before it only knew unhealthy, restart-looping and network-less containers, so a
  container stopped on purpose stayed down: `restart: unless-stopped` never restarts a stop, and `wf vm recover`'s OCI reboot does not
  either. A swap removes the old container within seconds, so it never trips it; another project's stopped container and a
  service dropped from the compose file are left alone. Flow tests: `backend/watchdog/rs/tests/flow.rs`.
- **Both drills have a service side** (D2b). `POST /drill?action=stop|freeze` is answered by the Rust service (the only vm-ops-api),
  with the signed token only (the legacy raw token is 401). It answers 202 `{accepted, action, container}`,
  the container being its own as the watchdog's `watchdog.recreated` line names it, and a detached sibling then runs
  `sleep 5; docker stop <container>`, or the freeze: `sleep 5; docker kill -s STOP`, `sleep 600`, `docker kill -s CONT || true`, so the
  thaw needs neither wf, nor 8080, nor a sandbox. `wf vm ops drill stop|freeze` (`clis/deploy/src/vm_drill.rs`) asserts `/health` on
  8080 stops answering, waits for it, reads who brought it back from the watchdog's own line and, if nothing does within ten
  minutes, reports `recovered: false`. Cases: `backend/vm-ops-api/rs/tests/flow/drill.rs`, `clis/deploy/tests/flow/vm_drill.rs`.

There is no standby and no fallback (owner decision, 23 Oct 2026): a recreate runs the same image, so a bad image is fixed by the next
push (the swap restores the previous files and tag when the new stack does not become healthy), and when the service stays down the whole
VM is recreated with Terraform, the recovery that none of this drills.
