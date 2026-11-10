---
status: open
area: tailnet
paths:
  - "backend/*/docker-compose.prod.yml"
  - "infra/oracle-cloud/**/docker-compose*.yml"
  - "infra/oracle-cloud/agent-shell/entrypoint.sh"
  - "infra/oracle-cloud/agent-shell/redeploy-app.sh"
  - "clis/deploy/src/tailscale.rs"
  - "clis/harness-browser/src/browser_tunnel.rs"
opened: 2026-08-20
last_verified: 2026-10-20
links:
  - archive/solutions/integration-issues/tailscale-stale-device-hostname-suffix.md
  - archive/solutions/integration-issues/tailnet-hostname-is-not-the-name-that-resolves.md
---
# Tailnet hostnames drift to a `-N` suffix on redeploy and nothing asserts the name

## What is broken

When a redeploy registers a sidecar before the previous ephemeral device is reaped, Tailscale hands
out `<name>-2`, permanently for that device; the configured name then resolves to nothing. It broke
Telegram for 51 silent hours (2026-08-18..20) and five vm-gateway targets, and recurred 2026-08-28 for
`remote-chat-agent-prod`. The fall-forward loops in agent-shell's `entrypoint.sh` (which tried
`-2..-5`) and in the deleted TypeScript agent-shell ssh host lookup were removed, and vm-ops-api's
dev-bridge resolver (`-1`..`-5` for the Metro's backend peers) left with the dev-bridge on 22 Oct. Two fall-forwards
remain: agent-shell's `redeploy-app.sh`, which tries `vm-ops-api-<label>`, then `-1`..`-3`; and
`wf browser tunnel`'s `live_host` (`clis/harness-browser/src/browser_tunnel.rs`), which looks a peer's requested
HostName up in `tailscale status --json` and connects to its live, possibly suffixed, DNSName.
`wf vm net device list` (`clis/deploy/src/tailscale.rs`) warns about suffixed devices, and
`wf vm net device retire --apply` deletes stale device records (dry run by default) so the next node
can claim the bare name; names are reclaimed by a `?wipeState=1` redeploy.

## Why blocked

The systemic fix was never chosen: non-ephemeral keys with `--reset`, addressing peers by IP or tag,
or a startup assertion that `tailscale status --self` reports the requested hostname.

## What unblocks it

Add the startup assertion to every tailscale sidecar (fail loud on a suffixed name) and pick one of
the structural options; verify by forcing a racing redeploy and seeing an immediate, logged failure
instead of a silent `-2`.

## Re-checked 2026-10-20

The gap still holds: no tailscale sidecar checks the name it registered under. All six compose files with TS_HOSTNAME only health-check `tailscale status --json`. The fall-forwards in the since-deleted vm-ops-api resolver (-1..-5), redeploy-app.sh (-1..-3) and browser_tunnel.rs `live_host` were unchanged then. Since 7 Oct and 17 Oct, a VM replace or `wf vm terraform apply --recreate` retires the old VM's tailnet records before the new nodes register. That covers the VM-recreate race but not a redeploy race. On 17 Oct a raw `-replace` still brought every service back as `<name>-1`. Today all 12 tailnet devices are on bare names.

Out of date above:

- The gap names no mitigation, but since f3a0af69b (7 Oct) and 4ce1efc4c/7ad8603b9 (17 Oct) a VM replace or `wf vm terraform apply --recreate` deletes the old VM's tailnet records before the new nodes boot (`retire_vm_records`, clis/deploy/src/infra.rs). Only the redeploy race is unguarded now.
- "It broke Telegram ... and five vm-gateway targets" is history only. The Telegram bot was deleted in fef439c0c (4 Oct) and vm-gateway in 260f45844 (6 Oct). Today the bare names are dialled by CI/wf (vm-ops-api-<label>), the dev-bridge (devBridge.ts) and the shared Metro link `exp://metro-runner-main:20000`. Note that clis/deploy/src/tailscale.rs:220 still warns that "vm-gateway forwards to the BARE hostname".

## Re-checked 2026-10-23

The dev-bridge resolver and the shared Metro link `exp://metro-runner-main:20000` are gone (22 Oct), so the bare names are dialled by CI and `wf` (`vm-ops-api-<label>`) and by the backends' own sidecars. Two fall-forwards remain, as above: `redeploy-app.sh` (`vm-ops-api-${HOST_LABEL}` then the suffixes) and `wf browser tunnel`'s `live_host`. `clis/deploy/src/tailscale.rs` still warns that "vm-gateway forwards to the BARE hostname" although vm-gateway is deleted (6 Oct); the warning's reason is stale, the suffix check it makes is not.
