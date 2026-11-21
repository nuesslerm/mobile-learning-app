---
title: Hetzner stock - the Server Radar alert that tells the owner when a cost-optimized server becomes orderable
verifies: A CX or CAX plan that flips to orderable in an EU location reaches the owner's inbox from Server Radar; `wf vm stock` answers the same question on demand.
tier: manual
---

# Hetzner stock

Oracle is gone and the cost-optimized Hetzner plans (CX x86, CAX Arm) have been unavailable, so the owner waits for stock. Since 19 Nov 2026 the signal is a Cloud availability alert on [Server Radar](https://radar.iodev.org) (open source: [elsbrock/hetzner-radar](https://github.com/elsbrock/hetzner-radar)), which emails the owner when a watched plan becomes orderable. It replaced the "Hetzner stock clock (every 2 h)" scheduled task, which ran `wf vm stock` in the `schedules` session; that task is disabled, not deleted, so it can be turned back on if Radar goes away.

## The alert

| Field | Value |
| --- | --- |
| Name | `MLA host: CX33/CX43/CAX21/CAX31 EU` |
| Id | `ec7ea0d5-abf4-4d38-a73b-d73c8e5c5fc0` |
| Plans (Radar server type ids) | cx33 (115), cx43 (116), cax21 (93), cax31 (94) |
| Locations (Radar location ids) | fsn1 (1), nbg1 (2), hel1 (3) |
| Fires on | `available` |
| Notifies | email to the owner's Radar account |

The plans are the nearest sizes to the old VM (2 vCPU, 12 GB): 4 vCPU and 8 GB, or 8 vCPU and 16 GB. The US and Singapore locations are left out for latency. A Radar account holds at most 3 active Cloud alerts. The alert disarms after it fires; when the owner has not ordered by then, re-arm it or create a new one.

## Changing the alert

Radar has no personal API token. Alerts are managed through its MCP endpoint `https://radar.iodev.org/mcp` (Streamable HTTP; a browser GET answers 405, which is expected), whose alert tools (`cloud_availability`, `list_cloud_alerts`, `create_cloud_alert`, `delete_cloud_alert`) need an OAuth access token. Sign-in is an email code, so a person has to approve it once per session:

1. Register a public client at `POST /api/auth/mcp/register` with `token_endpoint_auth_method: none` and the redirect URI `https://radar.iodev.org/mcp`, so the code never leaves Radar's own host.
2. Build an `/api/auth/mcp/authorize` link with PKCE (S256), `scope=openid email offline_access`, `resource=https://radar.iodev.org/mcp` and `prompt=consent`, and send it to the owner as a markdown link.
3. The owner signs in, approves, lands on the 405 page and pastes the address bar back; check `state`, then exchange `code` with the verifier at `POST /api/auth/mcp/token`.
4. Call the tools with `Authorization: Bearer <access_token>`. `create_cloud_alert` takes numeric ids from `cloud_availability`, never names.

The access token lives for one hour and is kept only in the session scratchpad: it is not in the vault and not in the repository's MCP config, because a fixed bearer header would stop working within the hour.

## Checking on demand

`wf vm stock` still asks the Hetzner Cloud API directly (`GET /v1/server_types`, read-only token `HCLOUD_READ_TOKEN` from the vault, scope `ci-deploy`), independently of Radar:

```bash
wf vm stock --family cx,cax --min-vcpu 4 --min-ram-gb 8
```

- `HCLOUD_READ_TOKEN is not set` or `the token was refused`: the vault key is missing or revoked. The owner creates a new read-only token (Hetzner console, the project, Security, API tokens, permission Read) and runs `wf secret update HCLOUD_READ_TOKEN --value-stdin --apply`.
- `no server types in the answer: the API changed`: Hetzner changed the endpoint and the verb needs a fix.

## Notes

- Availability, whether from Radar or the Hetzner API, is Hetzner's own indicator and not a guarantee: an order can still fail, and new accounts may be limited. The first real order is the test.
- Nothing here orders a server: the owner picks the plan and does the account steps.
- CX23 (2 vCPU, 4 GB) is orderable now and is below the thresholds on purpose.
