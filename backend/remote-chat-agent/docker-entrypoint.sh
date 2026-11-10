#!/usr/bin/env bash
# Container entrypoint: MagicDNS resolver setup, shared workspace init and the privilege drop, then execs the real CMD.
# This container holds no GitHub credential.
set -euo pipefail

# Add Tailscale's MagicDNS resolver (100.100.100.100) to DNS, reachable
# because this container shares its network namespace with the `tailscale`
# sidecar (`network_mode: service:tailscale`) -- but NOT its mount namespace,
# so Docker still writes THIS container its own separate /etc/resolv.conf
# pointing at Docker's embedded resolver (127.0.0.11), which has never heard
# of MagicDNS. Confirmed live via a temporary /debug/dns-check route: this
# container's own resolv.conf was exactly `nameserver 127.0.0.11`, and
# EVERY `*.taila61461.ts.net` lookup failed with ENOTFOUND -- including this
# container's OWN hostname, and VNC_AUTH_LISTENER_URL, which is what
# surfaced this as claude's reactive-login flow failing with `fetch failed`.
# Sharing network namespace does not share resolv.conf; only rewriting it
# ourselves, once, before the app starts, fixes every consumer of it. Compose
# can't express this via `dns:` (rejected: "conflicting options: dns and the
# network mode" for a `network_mode: service:X` service), so it has to happen
# here, in the one place root still runs before the app.
#
# Confirmed live the hard way: an EARLIER version of this fix REPLACED
# resolv.conf with only `nameserver 100.100.100.100`, which fixed every
# *.ts.net lookup but broke every OTHER lookup -- BetterStack log delivery
# started failing with `getaddrinfo EAI_AGAIN s2648510...betterstackdata.com`
# on the very next redeploy, because this tailnet has no global nameservers
# configured, so Tailscale's resolver can't answer anything outside its own
# suffix. `resolv.conf` supports up to three `nameserver` lines tried in
# order on timeout/SERVFAIL (which is what an unconfigured query outside
# Tailscale's known suffixes returns, not a clean NXDOMAIN) -- so ADD
# 100.100.100.100 as the first of two, keep Docker's original 127.0.0.11
# as the fallback for everything else, rather than replacing it.
if [[ "$(id -u)" == "0" ]]; then
  {
    echo "nameserver 100.100.100.100"
    echo "nameserver 127.0.0.11"
    echo "search taila61461.ts.net"
    echo "options ndots:1"
  } > /etc/resolv.conf 2>/dev/null \
    || echo "[docker-entrypoint] WARNING: could not rewrite /etc/resolv.conf for MagicDNS -- *.ts.net lookups from this container will keep failing" >&2
fi

# ---------------------------------------------------------------------------
# Shared workspace init + privilege drop.
#
# Symmetric with agent-shell's entrypoint on purpose: EITHER container may be
# the first to start on a fresh VM, and docker creates a new named volume's
# root as `root:root 0755`, which a non-root process cannot create inside.
# Whichever one boots first initialises it; the second finds the work done.
#
# One uid (1001) owns everything on this volume rather than a shared gid with
# `umask 002` -- measured, with the rejected alternative, in
# docs/solutions/infra/sharing-a-docker-volume-across-containers-needs-one-uid.md
if [[ -d /workspace ]]; then
  if [[ "$(id -u)" == "0" ]]; then
    chown agent:agent /workspace 2>/dev/null || true
    chmod 775 /workspace 2>/dev/null || true
    # Tier-1 prefix dirs (tools/npm, tools/mise, tools/bun, tools/cargo,
    # tools/pipx) listed explicitly here too, matching agent-shell's
    # entrypoint.sh -- not relying on npm/bun/etc. to lazily create their own
    # prefix on first write. Whichever surface boots first on an empty volume
    # is the one that matters: agent-shell already listed them, so rca's
    # omission was invisible as long as agent-shell won that race. It would
    # not be invisible the day rca boots first.
    runuser -u agent -- mkdir -p /workspace/home /workspace/repos /workspace/worktrees \
      /workspace/tools/npm/bin /workspace/tools/mise /workspace/tools/bun \
      /workspace/tools/cargo /workspace/tools/pipx \
      /workspace/cache/pnpm /workspace/.agent-env 2>/dev/null \
      || echo "[docker-entrypoint] WARNING: could not initialise /workspace layout" >&2
    # `runuser` resets HOME from /etc/passwd and DISCARDS the value compose
    # sets -- verified directly: with `-e HOME=/workspace/home`, the dropped
    # process reported HOME=/home/agent. Left unfixed, opencode's session store
    # and ~/.claude would land on the container's ephemeral layer while looking
    # entirely correct. Making passwd itself the source of truth fixes it for
    # every launch path, not just this one.
    usermod -d /workspace/home agent 2>/dev/null || true

    # Tier 2 (KTD4/U4): replay the declarative dependency manifest, same
    # split as agent-shell's entrypoint -- apt as root (this context), npm/mise
    # via `runuser -u agent` so installed files land agent-owned rather than
    # root-owned on a volume everything else here writes to as `agent`. Never
    # allowed to fail the boot: a missing optional tool is strictly better
    # than a container that won't start.
    wf vm deps replay --managers apt \
      || echo "[docker-entrypoint] WARNING: agent-deps replay (apt) reported a failure -- see the JSON result above" >&2
    runuser -u agent -- env NPM_CONFIG_PREFIX=/workspace/tools/npm wf vm deps replay --managers npm,mise \
      || echo "[docker-entrypoint] WARNING: agent-deps replay (npm,mise) reported a failure -- see the JSON result above" >&2
  fi
fi

# Drop to `agent` (1001) for the actual service. Deliberately NOT a `USER`
# directive in the Dockerfile: the volume init above needs root, and it has to
# happen after the volume is mounted, which only the entrypoint can do.
#
# This also retires the reason IS_SANDBOX=1 exists -- the Agent SDK refuses
# `permissionMode: 'bypassPermissions'` only when the process is root. Running
# non-root removes the need for that escape hatch instead of relying on it.
# Gated on /workspace EXISTING, not merely on being root -- this image is
# published to the same `latest` tag the VM pulls by default, so it must behave
# identically under the OLD compose file (which mounts no /workspace and points
# WORKTREE_BASE_DIR at a root-owned volume). Dropping to 1001 there would leave
# the service unable to write its own worktrees: a silent, delayed breakage
# triggered by nothing more than a routine redeploy picking up `latest`.
# With this gate, image and compose can land in either order.
# Hand back anything root created in the SHARED home during this boot.
#
# Found by deploying, not by reading: `gh auth setup-git` (since removed) ran above as root
# with HOME=/workspace/home, which left `.gitconfig` and `.local/` owned by
# root:root 0755 on a volume where everything else is 1001. opencode (also
# 1001, in its own container) then crash-looped on
#   EACCES: permission denied, mkdir '/workspace/home/.local/share'
# because it cannot create inside a root-owned directory. The uid model only
# holds if root NEVER leaves anything behind it owns.
#
# Scoped to entries root actually owns rather than a blanket `chown -R`: the
# shared home can grow large, and rewriting ownership across all of it on every
# boot is both slow and a good way to clobber something deliberate.
if [[ -d /workspace/home ]] && [[ "$(id -u)" == "0" ]]; then
  find /workspace/home -user root -exec chown agent:agent {} + 2>/dev/null || true
fi

if [[ -d /workspace ]] && [[ "$(id -u)" == "0" ]] && id -u agent >/dev/null 2>&1; then
  exec runuser -u agent -- "$@"
fi

exec "$@"
