#!/bin/bash
set -euo pipefail

# One image, four services (docs/archive/plans/persistent-session-services/contract.md
# section 1): AGENT_SHELL_ROLE selects which surface this entrypoint brings up.
# Unset/`all` = today's single combined container, line for line (a legacy
# deploy can never regress); `sessions` = sshd only, `dind` = nested dockerd
# only, `tools` = shared workspace + agent-deps replay + t3code. Every step a
# split role runs, `all` also runs, in today's exact order.
AGENT_SHELL_ROLE="${AGENT_SHELL_ROLE:-all}"
export AGENT_SHELL_ROLE

role_has() {
  [ "$AGENT_SHELL_ROLE" = all ] && return 0
  for role in "$@"; do
    [ "$AGENT_SHELL_ROLE" = "$role" ] && return 0
  done
  return 1
}

# Add Tailscale's MagicDNS resolver (100.100.100.100) to DNS, keeping the
# original Docker-embedded resolver (127.0.0.11) as a fallback for everything
# else. Root-caused in backend/remote-chat-agent this session (see
# docs/known-gaps.md): `network_mode: service:tailscale` shares NETWORK
# namespace with the sidecar but not MOUNT namespace, so Docker still writes
# this container its own separate /etc/resolv.conf pointing only at
# 127.0.0.11, which has never heard of MagicDNS -- every `*.taila61461.ts.net`
# lookup fails with ENOTFOUND, including VNC_AUTH_LISTENER_URL, which this
# container calls directly. Two nameservers, not a replacement:
# an earlier version of this same fix elsewhere replaced resolv.conf
# entirely and broke every non-tailnet lookup (confirmed live via a failed
# BetterStack log delivery) -- Tailscale's resolver returns SERVFAIL/timeout
# (not a clean NXDOMAIN) for anything outside its own suffix on a tailnet
# with no global nameservers configured, which is exactly the case glibc's
# resolver falls through to the second nameserver for.
if role_has sessions dind tools; then
  if [[ "$(id -u)" == "0" ]]; then
    {
      echo "nameserver 100.100.100.100"
      echo "nameserver 127.0.0.11"
      echo "search taila61461.ts.net"
      echo "options ndots:1"
    } > /etc/resolv.conf 2>/dev/null \
      || echo "entrypoint: WARNING: could not rewrite /etc/resolv.conf for MagicDNS -- *.ts.net lookups from this container will keep failing" >&2
  fi
fi

# authorized_keys is built from env at container START, not baked into the
# image -- so a key rotation is a redeploy (env/.env change), not a rebuild.
# SSHID_URL is the primary source: a deliberate decision to make sshid.io the
# single trusted PERSONAL identity source for this container, rather than
# also trusting the separate ORACLE_DEPLOY_SSH_PUBLIC_KEY/BREAK_GLASS_SSH_PUBLIC_KEY
# vault keys (those remain real secrets for the VM host's own Terraform var,
# just no longer wired in here). If SSHID_URL is ever unreachable at
# container start, this container has NO personal authorized key at all
# until the next successful start -- an explicit, accepted trade for "one
# source, rotated with no vault write", not an oversight.
#
# AGENT_SHELL_E2E_SSH_PUBLIC_KEY is the ONE deliberate exception to "SSHID_URL
# only" -- a narrowly-scoped, non-secret, machine-only test identity for an
# agent session's automated/exploratory e2e checks (see the harness-cli-login
# plan doc), the same "one deliberate exception" pattern this container
# already uses for exec-script. It is NOT a second personal identity.
# Stored base64-encoded in .env.defaults (see that file's own comment) --
# confirmed live that the raw "ssh-ed25519 <b64> <comment>" form, which
# contains spaces, never reaches this container's environment at all
# (silently dropped somewhere upstream); base64 keeps it a single
# space-free token end to end, decoded only here.
if role_has sessions; then
  mkdir -p /etc/ssh/authorized_keys.d
  {
    if [ -n "${SSHID_URL:-}" ]; then
      curl -fsS --max-time 10 "$SSHID_URL" || echo "entrypoint: SSHID_URL fetch failed, continuing without it" >&2
    fi
    [ -n "${AGENT_SHELL_E2E_SSH_PUBLIC_KEY:-}" ] && echo "$AGENT_SHELL_E2E_SSH_PUBLIC_KEY" | base64 -d
  } > /etc/ssh/authorized_keys.d/agent
  chmod 600 /etc/ssh/authorized_keys.d/agent
  chown agent:agent /etc/ssh/authorized_keys.d/agent
fi

# ---------------------------------------------------------------------------
# Shared workspace volume (agent-workspace, see docker-compose.prod.yml)
#
# Docker creates a fresh named volume's root as `root:root 0755`, so a non-root
# container cannot create ANYTHING at the top level -- measured, not assumed:
# `mkdir /workspace/home` as uid 1001 fails with EACCES on an untouched volume.
# This step runs while the entrypoint is still root, before it drops to sshd,
# which makes it the natural place to fix that once.
#
# One uid owning everything is the deliberate choice over a shared gid +
# setgid + `umask 002`: the umask variant works only while EVERY writer
# remembers it, and a writer that forgets produces a file the other side
# cannot write next to a directory it still can remove -- a half-broken tree
# rather than a clean failure. Full measurements and the rejected alternative:
# docs/solutions/infra/sharing-a-docker-volume-across-containers-needs-one-uid.md
#
# Tolerant of the volume being absent so the image still runs standalone.
if role_has dind tools && [ -d /workspace ]; then
  chown agent:agent /workspace 2>/dev/null || true
  chmod 775 /workspace 2>/dev/null || true
  # `runuser` rather than a root mkdir: anything created here must be owned by
  # `agent` from the start, and creating as root then chowning -R would rewrite
  # ownership across every worktree on the volume on every single boot.
  runuser -u agent -- mkdir -p \
    /workspace/home \
    /workspace/repos \
    /workspace/worktrees \
    /workspace/tools/npm/bin \
    /workspace/tools/mise \
    /workspace/tools/bun \
    /workspace/tools/cargo \
    /workspace/tools/pipx \
    /workspace/tools/t3code \
    /workspace/cache/pnpm \
    /workspace/.agent-env 2>/dev/null || \
    echo "entrypoint: could not initialise /workspace layout -- check volume ownership" >&2

  # sshd reads the login user's home from /etc/passwd, NOT from the HOME env
  # var compose sets (that only reaches PID 1 and its direct children). Without
  # this, an interactive SSH session would still land in the image-local
  # /home/agent and see none of the shared state -- which is the entire point
  # of the volume. Safe now that authorized_keys no longer resolves through %h.
  usermod -d /workspace/home agent 2>/dev/null || true

  # Tier 2 (KTD4/U4): replay the declarative dependency manifest so anything
  # installed at runtime survives a volume loss or VM recreate, not just a
  # redeploy -- Tier 1's persisted prefixes above cover that already, so this
  # is a near-no-op except after a real loss. Split by manager so installed
  # files land owned by the right uid: apt as root (this context), npm/mise
  # via `runuser -u agent` so they don't land root-owned under a volume every
  # other tool here writes to as `agent`. Never allowed to fail the boot --
  # a missing optional tool is strictly better than a container that won't
  # start (the spec's own requirement), hence `|| true` on both.
  wf vm deps replay --managers apt || echo "entrypoint: agent-deps replay (apt) reported a failure -- see the JSON result above" >&2
  runuser -u agent -- env NPM_CONFIG_PREFIX=/workspace/tools/npm wf vm deps replay --managers npm,mise \
    || echo "entrypoint: agent-deps replay (npm,mise) reported a failure -- see the JSON result above" >&2
fi

# Ephemeral host keys, regenerated on every container start. Nothing
# persists across a redeploy anyway (no volume for /etc/ssh); a changed
# host key just means a normal "first connection" TOFU prompt for whatever
# ssh client connects next, not a security regression.
if role_has sessions; then
  ssh-keygen -A
fi

# sshd's own process env includes whatever `docker run -e`/compose
# `environment:` set (that's how redeploy-app's own VM_GATEWAY_AUTH_SEED/
# HOST_LABEL get here) -- but an interactive SSH login shell does NOT
# inherit sshd's env by default; PAM starts each session from a clean
# slate and only reads /etc/environment (confirmed live: `docker exec`
# saw VM_GATEWAY_AUTH_SEED fine, but a real `ssh agent@...` session
# reported it unset). Writing the values PAM's pam_env module already
# reads on every login is the standard fix, not a workaround.
if role_has sessions; then
{
  [ -n "${VM_GATEWAY_AUTH_SEED:-}" ] && echo "VM_GATEWAY_AUTH_SEED=${VM_GATEWAY_AUTH_SEED}"
  [ -n "${HOST_LABEL:-}" ] && echo "HOST_LABEL=${HOST_LABEL}"
  # Same PAM-doesn't-inherit-sshd-env issue as above -- `wf secret login --remote`
  # needs both to reach the VNC listener from an interactive SSH session.
  [ -n "${VNC_AUTH_TRIGGER_SECRET:-}" ] && echo "VNC_AUTH_TRIGGER_SECRET=${VNC_AUTH_TRIGGER_SECRET}"
  [ -n "${VNC_AUTH_OWNER_CHAT_ID:-}" ] && echo "VNC_AUTH_OWNER_CHAT_ID=${VNC_AUTH_OWNER_CHAT_ID}"
  # Same PAM-doesn't-inherit-sshd-env issue as above, for a value baked in
  # via the Dockerfile's own ENV rather than compose -- PID 1 (sshd) sees
  # it either way, but a NEW SSH login shell only reads /etc/environment,
  # not whatever env sshd itself was started with. Without this, `require(
  # 'playwright')` (a global npm install, not on Node's default resolution
  # path -- see the Dockerfile's own comment) works over `docker exec` but
  # not from a real interactive SSH session, which is the whole point of
  # this container.
  [ -n "${NODE_PATH:-}" ] && echo "NODE_PATH=${NODE_PATH}"
  # Same PAM-doesn't-inherit-sshd-env problem, for the shared-volume tool
  # prefixes compose sets. Without these a login shell would install into the
  # image's own ephemeral paths and lose everything on the next redeploy --
  # the exact failure the volume exists to end.
  for v in HOME NPM_CONFIG_PREFIX MISE_DATA_DIR BUN_INSTALL CARGO_HOME PIPX_HOME PIPX_BIN_DIR PNPM_STORE_DIR; do
    eval "val=\${$v:-}"
    [ -n "$val" ] && echo "$v=$val"
  done
  # Puts the persisted global-install bin dirs ahead of the image's own, so a
  # tool installed from a previous session wins over a stale baked-in copy.
  echo "PATH=/workspace/tools/npm/bin:/workspace/tools/bun/bin:/workspace/tools/cargo/bin:/usr/local/bin:/usr/local/sbin:/usr/bin:/usr/sbin:/bin:/sbin"
  # Same PAM-doesn't-inherit-sshd-env fix for the tmux server socket: an
  # interactive SSH session here multiplexes under /workspace/.tmux on the
  # shared volume rather than the container's own /tmp, so a tmux server
  # started in one login outlives the client and is reachable from the
  # other role containers' shells.
  [ "$AGENT_SHELL_ROLE" = sessions ] && echo "TMUX_TMPDIR=/workspace/.tmux"
} >> /etc/environment
fi

if role_has sessions dind tools; then
  mkdir -p /var/log/agent-shell
fi

# Detached (nohup + disown) BEFORE the exec below replaces this script's
# own process image -- exec does not kill already-backgrounded children
# regardless, but this makes the intent explicit rather than relying on
# that. Emits an immediate heartbeat on boot (proving the container came
# up), then one every AGENT_SHELL_HEARTBEAT_INTERVAL_SECONDS (default 900 =
# 15 min) thereafter -- see heartbeat.sh.
if role_has sessions; then
  nohup /usr/local/bin/agent-shell-heartbeat > /var/log/agent-shell/heartbeat.log 2>&1 &
  disown
fi

# T3 Code (github.com/pingdotgg/t3code) server -- an independent remote
# control surface for this same container, see t3code-serve.sh and
# README.md's "T3 Code remote access" section. Off by default
# (AGENT_SHELL_T3CODE_ENABLED unset/false) until explicitly enabled and
# verified; runs as `agent` (never root), same reasoning as
# docker-login.sh below -- it spawns opencode/claude/codex and makes real
# git commits on the user's behalf.
if role_has tools && [ "${AGENT_SHELL_T3CODE_ENABLED:-false}" = "true" ]; then
  nohup runuser -u agent -- /usr/local/bin/t3code-serve > /var/log/agent-shell/t3code.log 2>&1 &
  disown
fi

# Nested dockerd (Docker-in-Docker, see Dockerfile's own comment) --
# detached BEFORE the exec below replaces this script's own process image,
# same "make the intent explicit" reasoning as any other backgrounded init
# step.
#
# No `--iptables=false` (originally added as a precaution against
# dockerd's iptables rules conflicting with the tailscale sidecar's own
# netns ownership, never confirmed empirically): a docker-container buildx
# builder runs buildkit in its OWN container on nested dockerd's bridge
# network, a network hop `--iptables=false` never NATs -- confirmed live,
# every registry fetch from that builder timed out
# (`dial tcp ...:443: i/o timeout`) with the flag set. Removed so dockerd
# manages its own NAT/MASQUERADE rules for its bridge; the original
# tailscale-conflict worry was a precaution, never an observed conflict
# (different iptables chains: dockerd owns DOCKER/DOCKER-USER/its own
# POSTROUTING rules for docker0, tailscale owns ts-input/ts-forward).
# --storage-driver=fuse-overlayfs: this container's own root filesystem
# is already overlayfs (standard for most container hosts), and nested
# dockerd's default overlay2 storage driver tries to mount ANOTHER real
# overlayfs on top of it -- confirmed live, buildx build failed with
# "mount source: \"overlay\" ... err: invalid argument" (the OCI host's
# kernel doesn't support overlay-on-overlay in this configuration).
# `vfs` was the first fix tried and DOES avoid that error, but confirmed
# live to be extremely costly: vfs has no copy-on-write at all, so EVERY
# build instruction (COPY, RUN, even a trivial `cp`) does a full,
# non-deduplicated recursive copy of the ENTIRE accumulated layer stack --
# a real backend image build took over an hour and the builder's state
# volume grew past 40GB for what should be a few hundred MB of unique
# content. fuse-overlayfs solves the SAME overlay-on-overlay problem via
# FUSE instead of the kernel's native overlay driver, while still giving
# real copy-on-write semantics -- needs /dev/fuse, available under this
# container's own `privileged: true` (no extra device mapping needed).
if role_has dind; then
  # A hard VM reset leaves these in the writable layer, and dockerd refuses to start over a stale docker.pid.
  rm -f /var/run/docker.pid /var/run/docker/containerd/containerd.pid
  # The `dind` role binds only the shared dind-socket path (the other role
  # containers' DOCKER_HOST points here); `all` keeps today's default socket
  # so the legacy combined container is byte-for-byte unchanged.
  # The `dind` role execs dockerd as PID 1 at the very end instead (see the
  # role_has dind block below): a backgrounded-and-disowned dockerd leaves
  # `wait` with no job to wait for, so it returned immediately and the
  # container exited 0 in a restart loop.
  if [ "$AGENT_SHELL_ROLE" != dind ]; then
    nohup dockerd --storage-driver=fuse-overlayfs > /var/log/agent-shell/dockerd.log 2>&1 &
    disown
  fi
fi

# Runs as `agent` (not root) so ~/.docker/config.json lands in agent's own
# HOME -- see the script's own comment. Backgrounded since it polls for
# dockerd readiness rather than blocking container startup on it.
if role_has dind; then
  nohup runuser -u agent -- /usr/local/bin/docker-login.sh > /var/log/agent-shell/docker-login.log 2>&1 &
  disown
fi

if role_has sessions; then
  rm -f /run/sshd.pid
  exec /usr/sbin/sshd -D -e
fi

if [ "$AGENT_SHELL_ROLE" = dind ]; then
  # cgroup v2 evacuation (as the docker:dind entrypoint does): move every process out of the root
  # cgroup and delegate the controllers. Without it the nested containers get a threaded root with
  # no controllers and cgroup.procs writes fail with ENOTSUP, so redroid's init cannot start.
  if [ -f /sys/fs/cgroup/cgroup.controllers ]; then
    mkdir -p /sys/fs/cgroup/init
    xargs -rn1 < /sys/fs/cgroup/cgroup.procs > /sys/fs/cgroup/init/cgroup.procs 2>/dev/null || :
    sed -e 's/ / +/g' -e 's/^/+/' < /sys/fs/cgroup/cgroup.controllers > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null || :
  fi
  exec dockerd -H unix:///run/dind/docker.sock --storage-driver=fuse-overlayfs
fi

if role_has dind; then
  wait
fi

if role_has tools; then
  sleep infinity
fi
