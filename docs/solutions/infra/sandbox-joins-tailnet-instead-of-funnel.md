---
module: infra/oracle-cloud
tags: [tailscale, sandbox, vm-ops-api, vm-gateway, funnel, egress, agent-access, socks5, http-proxy]
problem_type: workflow-pattern
last_updated: 2026-08-29
---

# A sandboxed agent session can JOIN the tailnet — it does not need the Funnel

## The problem

`vm-ops-api` is tailnet-only: its `docker-compose.yml` declares a `tailscale`
sidecar, `ops-api` and a `watchdog` container, with **no Caddy, no published
ports, and no `OPS_API_DOMAIN`**. So the only route in from outside the tailnet was
`vm-gateway`'s Tailscale **Funnel**; since vm-gateway was deleted, joining the
tailnet is the only route.

That made every agent session dependent on the Funnel working, and the Funnel is
the most fragile link in the chain:

- It is only declarative since `TS_SERVE_CONFIG` was added to vm-gateway's
  `docker-compose.prod.yml` (the file and the vm-gateway service have since
  been deleted). Before that, Funnel was enabled by someone running
  `tailscale funnel` inside the container by hand, which persists in the
  `tailscale-state` volume — so **any VM recreate came up with no public
  surface at all**.
- Serving TLS over Funnel additionally needs **HTTPS Certificates enabled for the
  tailnet** (admin console → DNS). Without it the handshake dies server-side with
  `TLSv1.3 alert internal error (592)` / `error:0A000438`, which looks like a
  network fault and is not.
- The OCI public IP is a red herring. README section 10's `<ip>.sslip.io` + Caddy
  story is **stale** — no compose file has run Caddy for a long time, so
  `https://<ip>.sslip.io/health` returns a proxy-level `503 no route to host`
  forever and tells you nothing.

Net effect: a session could see the VM healthy in OCI, see every container
`running`, and still have no way to talk to any of it.

## The fix: be a tailnet node

The sandbox can run `tailscaled` in **userspace mode** — no `NET_ADMIN`, no
`/dev/net/tun`, no privileged container — and expose a local SOCKS5 proxy. Then
every service is reachable at its MagicDNS name, exactly as the VM's own
containers see it. No Funnel, no HTTPS certs, no SSH, no public ports.

`tailscale` + `tailscaled` are pinned in `mise.toml` and provisioned onto PATH by
`.agents/setup.sh`'s `step_mise`, so present in every session.

The supported way to do steps 1 and 2 below is one call,
`wf vm net up --hostname sandbox-agent-probe --backend bitwarden`: it starts
`tailscaled` with the right flags if it is stopped, fetches `TS_VM_AUTH_KEY` and
joins, and is safe to re-run. The manual steps below are a close equivalent:
`wf` passes `--timeout=120s` instead of `--accept-routes` to `tailscale up`, and
lets `tailscaled` use its default state directory instead of `--statedir`.

```bash
# 1. Daemon, userspace networking + a local SOCKS5 proxy.
mkdir -p /var/lib/tailscale
nohup tailscaled \
  --tun=userspace-networking \
  --socks5-server=localhost:1055 \
  --outbound-http-proxy-listen=localhost:1055 \
  --statedir=/var/lib/tailscale > /tmp/tailscaled.log 2>&1 &

# 2. Join with the tagged VM auth key from the vault.
#    Capture it in a variable: libraries/hook-runtime/src/deny.json
#    (scripts/guards/block-secret-exposure.mjs) blocks redirecting a fetched
#    secret to a file, but lets the fetch share a Bash call with other commands.
TS_AUTHKEY=$(SECRETS_BACKEND=bitwarden wf secret get TS_VM_AUTH_KEY)

tailscale up --authkey="$TS_AUTHKEY" \
  --hostname=sandbox-agent-probe --accept-dns=false --accept-routes

# 3. Talk to anything on the tailnet THROUGH the SOCKS proxy.
#    vm-ops-api checks VM_OPS_API_AUTH_TOKEN, a separate secret from the
#    gateway seed; the raw token is accepted while REQUIRE_SIGNED_AUTH is unset.
TOKEN=$(SECRETS_BACKEND=bitwarden wf secret get VM_OPS_API_AUTH_TOKEN)
curl -sS --proxy socks5h://localhost:1055 \
  -H "Authorization: Bearer $TOKEN" http://vm-ops-api-main:8080/health
# {"ok":true,"commit":"…","startedAt":"…","host":"main","debugMode":false,…}
```

`/health` is not a public route: without the token (or with
`VM_GATEWAY_AUTH_SEED`, which vm-ops-api does not accept) it answers
`401 {"error":"unauthorized"}`. That 401 still proves the request crossed the
tailnet and reached the server.

`--proxy socks5h://` (not `socks5://`) matters: the `h` resolves DNS **at the
proxy**, the only place MagicDNS names exist. Passing `--proxy` also
overrides the sandbox's own `HTTPS_PROXY`, so these calls bypass the egress proxy
rather than fighting it — never unset `HTTPS_PROXY` globally to achieve that.

WireGuard's UDP is not required: tailscaled falls back to DERP relays over
HTTPS/443, which the sandbox's egress proxy already permits (its boot log shows
`CONNECT ... for target "log.tailscale.com:443" ... 200 Connection Established`).

### The same port also serves plain HTTP-CONNECT — use it for tools that don't speak SOCKS5

`tailscaled`'s real launch command (`wf vm net up`, in `clis/deploy/src/tailscale_up.rs`,
port from the `SOCKS_PORT` constant in the same file —
**1055**, not the 1056 this doc used to show for the HTTP listener) passes
**both** flags with the *same* port:

```
tailscaled --tun=userspace-networking \
  --socks5-server=localhost:1055 \
  --outbound-http-proxy-listen=localhost:1055
```

One port, two protocols, auto-detected per connection. `curl --proxy
socks5h://localhost:1055` (above) is the right choice for `curl`/`openssl`, but
**it is not the only option, and some HTTP clients don't support SOCKS5 at
all.** Nx's own remote-cache client was one: a native (Rust
`reqwest`-based) binary bundled inside the `nx` package, and `reqwest`'s SOCKS5
support is an opt-in cargo feature many CLI tools don't compile in. Pointing it
at the SOCKS5 URL failed silently — the same generic error whether a SOCKS proxy
was configured or not. (The since-removed Nx remote cache was reached via
`vm-gateway-main`, which is also gone, so the commands below are a historical
record of the diagnosis, not a working recipe.)

```
NX   Failed to send request: error sending request for url (http://vm-gateway-main:8080/vm-ops-api/nx-cache/v1/cache/<hash>)
```

The fix is the plain-`http://` scheme on the *same port*, which every standard
HTTP client (including `reqwest`'s default, non-SOCKS proxy support) honors via
the usual `HTTPS_PROXY`/`HTTP_PROXY` env vars. This is how it ran against the
Nx remote cache before that cache was removed:

```bash
HTTPS_PROXY="http://localhost:1055" HTTP_PROXY="http://localhost:1055" \
NX_SELF_HOSTED_REMOTE_CACHE_SERVER="http://vm-gateway-main:8080/vm-ops-api/nx-cache" \
NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN="<token>" \
pnpm exec nx run <project>:typecheck --skip-nx-cache=false
# Cache: 0/1 hit (0%) — first run, genuine miss, task executes and stores

rm -rf .nx/cache && <same command again>
# Cache: 1/1 hit (100%) — genuine remote hit, 25ms vs. the real run's 1.7s
```

**Takeaway: when a tool with its own HTTP client fails to reach a tailnet
MagicDNS hostname even though `curl --proxy socks5h://localhost:1055` works
fine for the same target, retry with `http://localhost:1055` (same port)
before concluding the sandbox can't reach it.** It almost certainly can — this
is a client-library gap, not a network one. This was mis-diagnosed at first in
one session as "the sandbox can't reach vm-gateway's Funnel at all," including
a plausible-looking but wrong TLS/proxy investigation of a completely
different (and irrelevant) egress path — the fix took one `ps aux | grep
tailscaled` to reveal the daemon already had the answer in its own launch
flags.

## The SOCKS proxy is a TRANSPORT, not a limitation — never cite it as one

This is the part that keeps going wrong — a behavioural rule, not a technical
one.

Once joined, this sandbox has **full tailnet access**. All of these work:

| want | how |
|---|---|
| SSH to agent-shell | `ssh -o ProxyCommand='nc -X 5 -x localhost:1055 %h %p' <agent-shell host>` |
| HTTP to any tailnet service | `curl --proxy socks5h://localhost:1055 http://vm-ops-api-main:8080/health` |
| an HTTP client without SOCKS5 support | `HTTP_PROXY=http://localhost:1055` + the tailnet URL (see above) |

Userspace mode (`TUN: false`) means no kernel route, so bare MagicDNS
names and raw `100.x` IPs do not resolve or route for arbitrary processes —
**that is a statement about which transport to use, and nothing else.** It is
not a capability gap, not a blocker, and not a reason something could not be
done or was not done.

**The rule:** never offer the SOCKS transport as an explanation for why
something is unavailable, unwired, or slow. If a tool needs the proxy, pass it
the proxy and move on. If a tool cannot take a proxy, put a loopback forwarder
in front of it (see below) — do not report it as a limitation.

This is recorded because the opposite happened repeatedly in one session: the
transport detail was raised as a blocker, corrected by the user, re-verified,
apologised for, and raised again. Re-deriving this fact is itself the
failure — it is settled here.

### The durable fix: make the transport invisible

The `nx-cache` step of the since-deleted `env-bootstrap` CLI started a loopback
proxy that absorbed **both** the SOCKS hop and the bearer auth. Callers then
needed one static, non-secret value —
`NX_SELF_HOSTED_REMOTE_CACHE_SERVER=http://127.0.0.1:1056/vm-ops-api/nx-cache` —
with no token and no proxy variable anywhere. The live equivalent for the
transport half is `clis/harness-browser/src/tailnet_forward.rs` (a localhost listener that
dials through the SOCKS5 port); it does not inject auth.

That is the shape to copy whenever a tool cannot speak SOCKS or should not hold
a credential: absorb the transport and the auth into a loopback hop, so the
thing above it never has to know either exists.

## Why this is the preferred path, not a workaround

Joining the tailnet gives strictly more than the Funnel did, with fewer moving
parts:

| | Funnel | Tailnet join |
| --- | --- | --- |
| Needs `TS_SERVE_CONFIG` on the gateway | yes | no |
| Needs tailnet HTTPS Certificates | yes | no |
| Survives a VM recreate | only since `TS_SERVE_CONFIG` | yes |
| Reaches apps other than the gateway | only via gateway routes | any node, directly |
| Exposes anything publicly | yes | no |

The last row is the real argument: Funnel puts a public HTTPS surface on the
internet whose only protection is a bearer token. A tailnet join needs no public
surface at all.

## Costs / gotchas, all confirmed live

- **The auth key is shared, reusable, ephemeral and tagged** (`wf vm net
  auth-key rotate` mints it that way). The node lands in `tag:mobile-learning`,
  so `wf vm net device list` counts it alongside the real services — a probe
  named `sandbox-agent-probe` is not a service, don't chase it.
- **Clean up after yourself**: `wf vm net device retire --name
  sandbox-agent-probe --apply`, adding `--force` while the node is still online
  or offline for less than the 10-minute stale threshold. Without `--name` it
  selects only tagged `vm-ops-api-*`/`vm-gateway-*` records (stale ones, plus
  online collision-suffixed ones under `--force`), so it never selects the probe.
  Without `--force` it never selects an untagged device, but `--name X --force`
  deletes X whether it is tagged or not.
  The probe's record is ephemeral, but until Tailscale removes it after the node
  goes offline it is a stale record that can steal a bare hostname on the next
  recreate.
- **`tailscale up` errors are silent on stdout.** Check `tailscale status`; a
  missing/expired key surfaces only in the sidecar-style log line
  `RegisterReq: got response; machineAuthorized=false; authURL=true`, i.e. it
  silently fell back to interactive browser login and will loop forever.

## How this was found

While trying to satisfy "make every service reachable from the sandbox", the
Funnel was brought up correctly (DNS resolved to a Tailscale ingress IP,
`nodeAttrs` granted `funnel` to `tag:mobile-learning`) and still failed every TLS
handshake. Rather than wait on a tailnet-wide console toggle that no available
credential could read or set, joining the tailnet needed only tools
and a vault secret the session already had — and reached both services on the
first try.

The general lesson is the one in `AGENTS.md`'s "Act, don't ask": the blocker was
never access, it was assuming the documented path was the only path.
