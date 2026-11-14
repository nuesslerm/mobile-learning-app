---
title: Hetzner stock routine - the clock that tells the owner when a cost-optimized server becomes orderable
verifies: A CX or CAX plan that flips to orderable at Hetzner reaches the owner's phone within about 2 hours.
tier: routine
schedule: every 2 h (cron `22 */2 * * *`, "Hetzner stock clock (every 2 h)", fires into the persistent `schedules` session `session_01MdgrcdJGE5MkZS29xApR9c`)
---

# Hetzner stock routine

Oracle is gone and the cost-optimized Hetzner plans (CX x86, CAX Arm) have been unavailable, so the owner waits for stock. `wf vm stock` asks the Hetzner Cloud API (`GET /v1/server_types`, read-only token `HCLOUD_READ_TOKEN` from the vault, scope `ci-deploy`) which plans are orderable in which location. The clock fires it every 2 hours; anyone can run the same step on request.

The tick prompt:

```text
Scheduled tick: the Hetzner stock check is due. Do your standing steps for it (docs/runbooks/hetzner-stock.md; one-line reply).
```

Keep this file and the stored prompt the same.

## Standing steps

1. `git fetch origin dev` and read this file from `origin/dev`, so a newer version wins.
2. Run `wf vm stock --family cx,cax --min-vcpu 4 --min-ram-gb 8 --state .agents/tmp/hetzner-stock.json`. The state file lives in the session checkout and is not committed. The thresholds are the nearest sizes to the old VM (2 vCPU, 12 GB): CX33 and CAX21 (4 vCPU, 8 GB), CX43 and CAX31 (8 vCPU, 16 GB).
3. Read `changed` in the JSON. Notify the owner only when an entry has `now: true` (a plan became orderable), with one push line from `summary` that names the plan and its locations; a plan going unavailable again, or `firstRun`, is not a push. Do not order anything: the owner picks the plan and the account steps.
4. Reply with one line, the `summary` field. Commit nothing.

## When it cannot run

- `HCLOUD_READ_TOKEN is not set` or `the token was refused`: the vault key is missing or revoked. Say so in the reply line and the owner creates a new read-only token (Hetzner console, the project, Security, API tokens, permission Read) and runs `wf secret update HCLOUD_READ_TOKEN --value-stdin --apply`.
- `no server types in the answer: the API changed`: Hetzner changed the endpoint. Report it in the reply line; the verb needs a fix, a quiet clock is not a clear signal.
- The tool is not installed in the session: say so in the reply line and stop. The session has had no `wf` binary before; the owner or the sandbox bootstrap installs it.

## Notes

- `available` is Hetzner's own indicator and not a guarantee: an order can still fail, and new accounts may be limited. The first real order is the test.
- CX23 (2 vCPU, 4 GB) is orderable now and is below the thresholds on purpose.
