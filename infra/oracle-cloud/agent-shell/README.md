# agent-shell

An isolated SSH surface for interactive/agent use of `claude`/`codex`/`opencode`/`wf`,
deliberately separate from the VM host.

## Why this exists

The VM host's `ubuntu` user is in the `docker` group so it can run `docker
compose` for every app — which also means SSH access to the host is
effectively near-root (docker-group membership is a well-known
container-escape path: `docker run -v /:/host ... chroot /host`). Rotating
the deploy keypair (see `docs/known-gaps.md` #8/#9) doesn't change that
blast radius, only who holds the key.

`agent-shell` is a single-purpose container with:

- No `/var/run/docker.sock` mount, no `docker` CLI, no host bind-mount of
  any kind. A fully compromised session inside it has nothing to escalate
  to — it can't reach the host, the other apps' containers, or anything
  outside its own tailnet identity.
- A dedicated non-root `agent` user (no `sudo`, no root login).
- Redeploy capability for the VM's *other* apps via `redeploy-app <app>
  [tag]`, a thin wrapper around `vm-ops-api`'s own `POST
  /services/:app/redeploy` HTTP endpoint (see `../vm-ops-api/README.md`
  section 10) over the tailnet — never a local docker call. Scoped exactly
  the same way every other caller of that endpoint already is: it can only
  select an already-published image tag, never push novel code.
- Its own Tailscale sidecar, joined the same way every other app's is, plus
  a Funnel-exposed, TLS-terminated raw TCP forward of its sshd — reachable
  not just from Termius but from a *sandboxed agent session* (HTTPS-only
  egress), scoped to this dedicated container instead of the host.

This invariant (nothing to escalate to on the host) still holds even after
later additions (Docker-in-Docker, `redeploy-app`, `exec-script`) — but
those additions changed what a compromise of *this container specifically*
is actually worth, and shifted the real perimeter away from the SSH key
alone. See `docs/solutions/security/agent-shell-blast-radius-analysis.md`
for the full analysis and why the recommended next step is scoping
`VM_GATEWAY_AUTH_SEED` per purpose, not splitting this container.

## Connecting

### From Termius (a normal SSH client)

Authorized keys are sourced ONLY from `SSHID_URL` (`.env.defaults` —
currently `https://sshid.io/nuesslerm`), fetched fresh by `entrypoint.sh` on
every container start. Whichever key(s) that URL lists are the only ones
trusted here — there is no separate vault-managed key for this container
(deliberate: one source, rotated by editing the sshid.io identity with no
vault write and no redeploy... well, a redeploy IS still needed today, since
the fetch only happens at container start, not on a timer). Use the private
key matching whichever public key `SSHID_URL` currently serves:

```bash
ssh -p 443 agent@agent-shell-main.<tailnet>.ts.net
```

(The exact hostname is `agent-shell-<HOST_LABEL>.<tailnet-name>.ts.net` —
`tailscale funnel status` on the sidecar reports it.)

### From a sandboxed agent session (HTTPS-only egress, no raw SSH)

A `--tls-terminated-tcp` Funnel forward is a real TLS connection at the
transport layer, so it passes through a TLS-inspecting egress proxy the same
way any other HTTPS request does — unlike a raw `ssh` connection, which is
not TLS and gets dropped by exactly that kind of proxy (the identical failure
mode hits the *host's* sshd). Wrap
the connection in `openssl s_client` as an SSH `ProxyCommand`:

```bash
ssh -o "ProxyCommand=openssl s_client -quiet -connect agent-shell-main.<tailnet>.ts.net:443" \
    -i <path-to-private-key> agent@agent-shell-main.<tailnet>.ts.net
```

## A redeploy needs `?ref=` until this lands on `main`

`agent-shell`'s own directory (this README, its compose file, `.env.defaults`)
doesn't exist on `main` yet — it's only on this branch/PR. `POST
/services/agent-shell/redeploy` with no `ref` fetches from `main` by default
and 404s. Until this merges, pass the branch explicitly:

```bash
curl -X POST -H "Authorization: Bearer <token>" \
  "https://<vm-ops-api>/services/agent-shell/redeploy?ref=claude/codex-claude-oci-vm-4uhsb7"
```

Delete this section once the branch merges and a plain `/redeploy` works again.

## Enabling Funnel (one-time per fresh container/redeploy)

Not baked into a static `TS_SERVE_CONFIG` (unlike `vm-gateway`'s HTTPS
Funnel) — the exact JSON schema for a TLS-terminated TCP forward wasn't
worth guessing at when the imperative CLI form is already a proven, working
pattern (`cloud-init.sh.tftpl` already uses the equivalent imperative
`tailscale funnel --bg localhost:8080` for `vm-gateway`). Run once after the
sidecar is up:

```bash
docker compose -f docker-compose.prod.yml exec -T tailscale \
  tailscale funnel --bg --tls-terminated-tcp=443 tcp://localhost:22
```

Verify: `docker compose -f docker-compose.prod.yml exec -T tailscale tailscale funnel status`.

First-time Funnel enable on a fresh tailnet node identity may need the same
one-time account-level ACL/authorization click documented in
`../README.md` section 10 for every other Funnel-enabled app in this repo.

## Redeploying another app from inside this container

```bash
redeploy-app leetcode-backend sha-abc1234
redeploy-app remote-chat-agent   # defaults to :latest
```

## Docker-in-Docker + Playwright

This container runs its own nested `dockerd` (no host socket, no host
mount) and has Playwright + a real Chromium installed at
`PLAYWRIGHT_BROWSERS_PATH=/opt/pw-browsers`, the same way a Claude Code
Remote sandbox session gets both. `agent` is in the (nested) `docker`
group, so `docker build`/`docker run`/`docker push` all work without
`sudo`. `docker login` runs at container start with `DOCKERHUB_TOKEN`
(required: an unset or rejected token fails the start).

## Verifying `wf` itself, without SSH

`e2e-wf-smoke` (baked into this image, `infra/oracle-cloud/agent-shell/e2e-wf-smoke.sh`)
checks that the baked `wf` binary runs and prints its usage. Run it
directly once you're in:

```bash
e2e-wf-smoke
```

Or trigger it from outside the container entirely -- no SSH, no key --
as an exec-script job:

```bash
printf 'e2e-wf-smoke\n' > /tmp/smoke.sh && wf vm exec /tmp/smoke.sh
```

Re-run this after every redeploy that touches `wf` -- a redeploy is not "done" until this comes back `exitCode: 0`.

Both need `VM_PROVISION_GITHUB_TOKEN` (optional; the same Contents:Read
PAT vm-ops-api already uses) to `git clone` this private repo — without
it they fail with a plain auth error at the clone step.

## Ad-hoc script execution, without SSH or a redeploy

`e2e-wf-smoke` above
is baked into the image — a real, reviewable file in a real commit,
but the edit → rebuild → redeploy round trip takes several minutes per
iteration. For a fast debug/e2e loop where that's too slow, `vm-ops-api`
exposes `POST /services/agent-shell/exec-script`: POST any script body,
have it run immediately inside this container, and read its output back
over HTTP.

**This is the one deliberate exception** to this repo's "no generic exec
endpoint" rule (see `../README.md` section 10 and `vm-ops-api/server.js`'s
own top-of-file comment) — narrowed specifically to this container, whose
whole purpose already accepts arbitrary code execution as its threat model
(that's what SSH access here already grants). No other app container gets
this capability.

It's an **async job**, not one blocking call with a hard timeout: starting
a job returns immediately with a `jobId`; a status route reports whether
it's still running plus whatever output has appeared so far; a cancel
route can abort it on demand. A generous backstop `timeout` (30 min) still
exists purely so an abandoned job can't run forever unattended — it is not
how a hang is meant to be noticed.

```bash
# Start a job -- returns immediately
curl -X POST -H "Authorization: Bearer <token>" \
  --data-binary 'echo starting; sleep 20; echo done; exit 0' \
  "https://<vm-ops-api>/services/agent-shell/exec-script"
# {"jobId":"6366befe-f8f9-4214-9ead-151906c206eb"}

# Poll while it's still running -- partial output included
curl -H "Authorization: Bearer <token>" \
  "https://<vm-ops-api>/services/agent-shell/exec-script/6366befe-f8f9-4214-9ead-151906c206eb"
# {"jobId":"...","running":true,"exitCode":null,"output":"starting\n"}

# Poll again once it's finished
# {"jobId":"...","running":false,"exitCode":0,"output":"starting\ndone\n"}

# Abort a job that's hanging or just taking too long
curl -X POST -H "Authorization: Bearer <token>" \
  "https://<vm-ops-api>/services/agent-shell/exec-script/6366befe-f8f9-4214-9ead-151906c206eb/cancel"
# {"ok":true}  -- exitCode becomes 143 (SIGTERM) on the next poll
```

`wf vm exec <script>` wraps this end to end (starts a job,
polls, streams new output as it arrives, exits with the job's real code);
`--no-wait` plus `wf vm exec --status`/`exec-cancel` for managing
polling yourself.

The script body never becomes a shell string — it lands via the same
tar-archive-write path (`buildSingleFileTar`/`dockerPutArchive`)
`/agent-shell/credentials` already uses, with zero argv/quoting surface.
Cancellation and the backstop both kill the whole process tree via a
`setsid`-created process group, not just the top-level shell — verified
live against a real `sleep 300` that a naive parent-only kill left running
untouched the first time this was built (see `server.js`'s own comments on
the route for the three separate live-tested bugs that shape it: the
process-group target, the watchdog's own orphaned grandchild, and a
group-kill racing the very shell meant to record it).

## Installing dependencies, persistently, with no redeploy

Two persistence tiers already exist for exactly this, both driven through
`exec-script` above — neither needs a redeploy:

- **Tier 1 (automatic, no extra step).** `HOME`, `NPM_CONFIG_PREFIX`,
  `MISE_DATA_DIR`, `BUN_INSTALL`, `CARGO_HOME`, `PIPX_HOME`, and
  `PNPM_STORE_DIR` are all redirected onto the persistent `/workspace`
  volume (`docker-compose.prod.yml`). A plain `npm install -g <pkg>` /
  `mise install <tool>` / `cargo install <crate>` / `pipx install <pkg>`,
  run via `exec-script` or `wf vm exec`, survives a real redeploy
  with nothing else to do.
- **Tier 2 (apt, and everything else that has no persistent prefix).**
  `apt-get install` writes into the container's writable layer, which a
  real redeploy fully recreates (`docker compose ... down` then `up -d`,
  not a no-op) — so an apt-installed package is live immediately but does
  NOT survive a redeploy on its own. `wf vm deps` (already
  baked into this image, already invoked on every boot by `entrypoint.sh`
  via `agent-deps replay`) is the declarative manifest that closes this
  gap: `add` records an entry, `replay` reinstalls anything in the
  manifest that isn't already present.

`wf vm deps install <manager> <name>` (run from the sandbox, not
inside the container) composes both steps into one job — install, then
record — so a failed install never gets recorded as if it succeeded:

```bash
wf vm deps install apt ripgrep
# jobId: 6366befe-...
# (installs immediately, then runs `wf vm deps add apt ripgrep`)
```

`<manager>` is one of `apt`, `npm`, `mise`. For npm/mise this is a
belt-and-suspenders record on top of Tier 1's automatic persistence
(`agent-deps replay` skips anything already present); for apt it's the
only thing that makes the install outlive a redeploy at all.

Verify it actually persisted with `wf vm deps list`
(via `exec-script`) before and after a real redeploy — a manifest entry on
its own only proves it was recorded, not that replay can reinstall it.

## T3 Code remote access

[T3 Code](https://github.com/pingdotgg/t3code) (Theo/Julius's open-source
"agent harness control surface") is a second, independent remote control
surface for this container: its mobile/web/desktop app drives
`opencode`/`claude`/`codex` directly, as an alternative to going through
`remote-chat-agent`. It's off by default (`AGENT_SHELL_T3CODE_ENABLED`)
until enabled and verified — see `t3code-serve.sh` for the supervisor.

### Why tailnet-only, not Tailscale Serve/Funnel

T3 Code's own docs describe three remote-access modes: tailnet-direct
(`t3 serve --host <tailnet IP>`), `--tailscale-serve` (asks `tailscaled` to
reverse-proxy HTTPS via `tailscale serve`), and their own hosted "T3
Connect" tunnel. **This container can only use the first one as-is.**
`app` shares the sidecar's *network* namespace (`network_mode:
service:tailscale`) but not its *mount* namespace, and has no `tailscale`
CLI of its own baked in — `--tailscale-serve` needs to shell out to
`tailscale`/talk to `tailscaled`'s control socket, neither of which is
reachable from `app`. `t3code-serve.sh` instead binds directly to the
`tailscale0` interface's own address (reading it via `ip addr`, which
works because the network namespace itself is shared), which needs no CLI
at all and matches this container's existing "minimize exposure" design —
nothing here is reachable off the tailnet.

The tradeoff: the T3 Code client (phone, laptop) needs the Tailscale app
installed and joined to the same tailnet. If that's ever too inconvenient,
`--tailscale-serve` becomes viable by additionally installing a `tailscale`
CLI into the `app` image and bind-mounting the sidecar's control socket
directory into it — a real infra change, not a flag flip, so it's not done
by default.

### One-time setup

1. Install a second Node runtime (T3 Code's server needs
   `^22.16 || ^23.11 || >=24.10`; the repo-wide `mise.toml` pin, 22.14.0,
   fails that range) — persists across redeploys via the Tier 1/2 system
   above:
   ```bash
   wf vm deps install mise "node@${AGENT_SHELL_T3CODE_NODE_VERSION}"
   ```
2. Clone the project checkout T3 Code will point at — a dedicated,
   persistent clone under `/workspace/repos`, not a `wf`-managed worktree
   (that pool belongs to `remote-chat-agent`'s own worktree API):
   ```bash
   git clone https://x-access-token:${VM_PROVISION_GITHUB_TOKEN}@github.com/nuesslerm/mobile-learning-app.git \
     /workspace/repos/t3code-project
   ```
3. Authenticate whichever provider(s) you want T3 Code to drive, on this
   machine (not the client device) — already-solved, nothing T3-specific:
   `opencode auth login` (plain API-key providers, no browser) or
   `wf secret login opencode` (`clis/vnc-auth-capture`'s existing adapter,
   for OAuth-style providers) for OpenCode; `wf secret login claude` for
   Claude. This keeps the repo's Anthropic-credential policy intact —
   OpenCode never receives an `ANTHROPIC_*` credential either way, since
   Claude models go through T3 Code's separate native Claude provider.

### Enabling

Set in `.env.defaults` (or override via `.env.local`), then redeploy:

```bash
AGENT_SHELL_T3CODE_ENABLED=true
```

### Verifying

Direct interface check first (SSH in): confirm the process
(`cat /var/log/agent-shell/t3code.pid`, `kill -0 <pid>`), tail
`/var/log/agent-shell/t3code.log` for the printed pairing URL/QR, and
confirm `T3CODE_HOME`'s `userdata/state.sqlite` exists.

Then the real entry-point check: pair an actual T3 Code client with that
URL, add `/workspace/repos/t3code-project` as a project, start a session
with OpenCode as the provider, make one trivial change, and confirm it
landed as a real commit (`git -C /workspace/repos/t3code-project log -1`)
and that T3 Code's own per-turn checkpoint ref exists
(`git -C /workspace/repos/t3code-project for-each-ref refs/t3/checkpoints`).

### Disabling / rollback

`AGENT_SHELL_T3CODE_ENABLED=false` + redeploy stops it with no data loss —
`/workspace/tools/t3code` and the project checkout are untouched, so
re-enabling later is instant. To remove entirely: also
`rm -rf /workspace/tools/t3code /workspace/repos/t3code-project` and
`wf vm deps remove mise "node@${AGENT_SHELL_T3CODE_NODE_VERSION}"`
(leaves the repo-wide 22.14.0 pin untouched — this only ever added a
second, independent version).
